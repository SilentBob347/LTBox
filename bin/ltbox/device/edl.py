import subprocess
import time
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Callable, List, Optional

import serial

from .. import constants as const
from ..errors import DeviceCommandError
from ..i18n import get_string
from ..ui import ui
from .support import (
    BaseDeviceManager,
    DeviceCommandRunner,
    find_edl_port,
    format_serial_port_bare,
    prevent_sleep_during_flash,
)

_ERASE_LABELS = frozenset({"userdata", "metadata", "frp"})
_FHLOADER_ERASE_FILENAME = "FHLoaderErase.xml"


class EdlManager(BaseDeviceManager):
    """EDL device manager using qdl-rs."""

    def __init__(
        self,
        usb_port_hint: Optional[Callable[[], None]] = None,
        command_runner: Optional[DeviceCommandRunner] = None,
    ):
        super().__init__(usb_port_hint=usb_port_hint, command_runner=command_runner)

    def check_device(self, silent: bool = False) -> Optional[str]:
        if not silent:
            ui.info(get_string("device_check_edl"))

        try:
            port_name = find_edl_port()
            if port_name:
                if not silent:
                    ui.info(get_string("device_found_edl").format(device=port_name))
                return port_name

            if not silent:
                ui.warn(get_string("device_no_edl"))
                ui.warn(get_string("device_connect_edl"))
            return None
        except serial.SerialException as e:
            if not silent:
                ui.error(get_string("device_err_check_edl").format(e=e))
            return None

    def wait_for_device(self) -> str:
        self._usb_port_hint()
        port_name = self.check_device(silent=True)
        if port_name:
            ui.info(get_string("device_edl_connected").format(port=port_name))
            return port_name

        try:
            from .. import utils

            with ui.status(get_string("device_wait_edl_loop")):
                port_name = utils.wait_for_condition(
                    lambda: self.check_device(silent=True),
                    interval=2.0,
                )
            ui.info(get_string("device_edl_connected").format(port=port_name))
            return port_name
        except KeyboardInterrupt:
            ui.warn(get_string("device_wait_edl_cancel"))
            raise

    def _run_command(
        self,
        command: list[str],
        *,
        cwd: Optional[Path] = None,
        check: bool = True,
        timeout: Optional[float] = None,
        capture: bool = False,
    ):
        return self._command_runner.run(
            command,
            capture=capture,
            check=check,
            cwd=cwd,
            timeout=timeout,
        )

    def _ensure_edl_port(self, port: str, timeout: float = 45.0) -> str:
        """Wait for a stable EDL port to appear after a qdl-rs reset.

        qdl-rs resets the device to EDL after each operation. The old COM
        port may linger briefly before USB disconnect, and the post-reset
        port may flap while Windows re-enumerates. Two phases:

        1. If a port is visible right after the grace period, watch up to
           5s for it to disconnect. If it stays, it is not a stale remnant
           and we trust it.
        2. Otherwise, poll until the same port is observed on two
           consecutive checks (1s apart) — that is the new stable port.

        Raises DeviceCommandError on timeout so callers never spawn
        qdl-rs against a vanished COM device.
        """
        deadline = time.monotonic() + timeout
        # Grace period for the reset cycle to begin tearing down the port.
        time.sleep(2.0)

        initial = find_edl_port()
        if initial is not None:
            disconnect_deadline = min(deadline, time.monotonic() + 5.0)
            saw_disconnect = False
            while time.monotonic() < disconnect_deadline:
                time.sleep(1.0)
                if find_edl_port() is None:
                    saw_disconnect = True
                    break
            if not saw_disconnect:
                return initial

        last: Optional[str] = None
        while time.monotonic() < deadline:
            time.sleep(1.0)
            current = find_edl_port()
            if current is not None and current == last:
                return current
            last = current

        raise DeviceCommandError(
            get_string("device_err_edl_port_timeout").format(
                port=port, timeout=int(timeout)
            )
        )

    def _base_cmd(self, port: str, loader_path: Path) -> list[str]:
        return [
            str(const.QDLRS_EXE),
            "--backend",
            "serial",
            "-d",
            format_serial_port_bare(port),
            "-l",
            str(loader_path),
            "-s",
            "ufs",
        ]

    def load_programmer(self, port: str, loader_path: Path) -> None:
        if not const.QDLRS_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_qdlrs_missing").format(path=const.QDLRS_EXE)
            )

        cmd = self._base_cmd(port, loader_path) + ["nop"]
        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd, timeout=30.0)
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            msg = get_string("device_fatal_programmer")
            msg += f"\n{get_string('device_fatal_causes')}"
            msg += f"\n{get_string('device_cause_1')}"
            msg += f"\n{get_string('device_cause_2')}"
            msg += f"\n{get_string('device_cause_3')}"
            msg += f"\nError: {e}"
            raise DeviceCommandError(msg, e)

    def load_programmer_safe(self, port: str, loader_path: Path) -> None:
        ui.info(get_string("device_upload_programmer").format(port=port))
        self.load_programmer(port, loader_path)

    def read_partition(
        self,
        port: str,
        output_filename: str,
        lun: str,
        start_sector: str,
        num_sectors: str,
        memory_name: str = "UFS",
        *,
        partition_name: Optional[str] = None,
    ) -> None:
        if not const.QDLRS_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_qdlrs_missing").format(path=const.QDLRS_EXE)
            )

        dest_file = Path(output_filename).resolve()
        dest_dir = dest_file.parent
        dest_dir.mkdir(parents=True, exist_ok=True)

        loader_path = const.CONF.edl_loader_file
        port = self._ensure_edl_port(port)

        name = partition_name or dest_file.stem
        cmd = self._base_cmd(port, loader_path) + [
            "-L",
            str(lun),
            "dump-part",
            "-o",
            str(dest_dir),
            name,
        ]

        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd, cwd=dest_dir)
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(get_string("device_err_edl_read").format(e=e), e)

        # qdl-rs saves with the partition name; rename to the expected filename
        if not dest_file.exists():
            for candidate in dest_dir.glob(f"{name}.*"):
                if candidate.is_file() and candidate != dest_file:
                    candidate.rename(dest_file)
                    break
            else:
                # No extension variant — check bare name
                bare = dest_dir / name
                if bare.exists() and bare.is_file() and bare != dest_file:
                    bare.rename(dest_file)

    def write_partition(
        self,
        port: str,
        image_path: Path,
        lun: str,
        start_sector: str,
        memory_name: str = "UFS",
        *,
        partition_name: Optional[str] = None,
    ) -> None:
        if not const.QDLRS_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_qdlrs_missing").format(path=const.QDLRS_EXE)
            )

        image_file = Path(image_path).resolve()
        loader_path = const.CONF.edl_loader_file
        port = self._ensure_edl_port(port)

        name = partition_name or image_file.stem
        cmd = self._base_cmd(port, loader_path) + [
            "-L",
            str(lun),
            "write",
            name,
            str(image_file),
        ]

        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd)
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(get_string("device_err_edl_write").format(e=e), e)

    def reset(self, port: str, *, mode: str = "system") -> None:
        if not const.QDLRS_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_qdlrs_missing").format(path=const.QDLRS_EXE)
            )

        loader_path = const.CONF.edl_loader_file
        port = self._ensure_edl_port(port)
        cmd = self._base_cmd(port, loader_path) + [
            "--reset-mode",
            mode,
            "reset",
            mode,
        ]
        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd, timeout=30.0, check=False)
        except FileNotFoundError as e:
            raise DeviceCommandError(get_string("device_err_reset_fail").format(e=e), e)

    def reset_to_edl(self, port: str) -> None:
        """Reset device back to EDL mode (stays in Sahara, no system reboot)."""
        self.reset(port, mode="edl")

    def _is_wipe_erase_label(self, label: str) -> bool:
        normalized = label.strip().lower()
        return any(
            normalized == base or normalized.startswith(f"{base}_")
            for base in _ERASE_LABELS
        )

    def _build_fhloader_erase_entries(self, raw_xmls: List[Path]) -> list[ET.Element]:
        erase_entries: list[ET.Element] = []
        seen_entries: set[tuple[tuple[str, str], ...]] = set()

        for xml_path in raw_xmls:
            try:
                tree = ET.parse(xml_path)
            except (ET.ParseError, OSError) as e:
                raise RuntimeError(
                    f"Failed to parse '{xml_path.name}' while building "
                    f"{_FHLOADER_ERASE_FILENAME}: {e}"
                ) from e

            for prog in tree.getroot().findall(".//program"):
                label = prog.get("label", "")
                if not self._is_wipe_erase_label(label):
                    continue

                physical_partition_number = prog.get(
                    "physical_partition_number", ""
                ).strip()
                start_sector = prog.get("start_sector", "").strip()
                num_partition_sectors = prog.get("num_partition_sectors", "").strip()

                if not (
                    physical_partition_number and start_sector and num_partition_sectors
                ):
                    raise RuntimeError(
                        f"'{label or 'unknown'}' entry in '{xml_path.name}' is "
                        f"missing erase geometry required to build "
                        f"{_FHLOADER_ERASE_FILENAME}."
                    )

                try:
                    if int(num_partition_sectors) == 0:
                        continue
                except ValueError as e:
                    raise RuntimeError(
                        f"'{label}' entry in '{xml_path.name}' has invalid "
                        f"num_partition_sectors='{num_partition_sectors}'."
                    ) from e

                erase_attrib_items = tuple(
                    sorted(
                        (key, value)
                        for key, value in prog.attrib.items()
                        if key != "filename"
                    )
                )
                if erase_attrib_items in seen_entries:
                    continue
                seen_entries.add(erase_attrib_items)

                erase = ET.Element("erase")
                for key, value in prog.attrib.items():
                    if key != "filename":
                        erase.set(key, value)
                erase_entries.append(erase)

        if erase_entries:
            return erase_entries

        raise FileNotFoundError(
            f"Missing userdata/metadata/frp erase spans required to build "
            f"{_FHLOADER_ERASE_FILENAME}."
        )

    def _build_fhloader_erase_xml(self, work_dir: Path, raw_xmls: List[Path]) -> Path:
        erase_root = ET.Element("data")
        for erase_entry in self._build_fhloader_erase_entries(raw_xmls):
            erase_root.append(erase_entry)

        erase_xml = work_dir / _FHLOADER_ERASE_FILENAME
        erase_tree = ET.ElementTree(erase_root)
        ET.indent(erase_tree, space="  ")
        erase_tree.write(erase_xml, encoding="utf-8", xml_declaration=True)
        return erase_xml

    def flash_rawprogram(
        self,
        port: str,
        loader_path: Path,
        memory_type: str,
        raw_xmls: List[Path],
        patch_xmls: List[Path],
        *,
        pre_erase: bool = False,
        reset_after: bool = False,
    ) -> None:
        if not const.QDLRS_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_qdlrs_missing").format(path=const.QDLRS_EXE)
            )

        ui.info(get_string("device_step1_load"))
        self.load_programmer_safe(port, loader_path)

        try:
            program_xmls = list(raw_xmls)
            if pre_erase:
                erase_xml = self._build_fhloader_erase_xml(
                    loader_path.parent, program_xmls
                )
                program_xmls.insert(0, erase_xml)

            with prevent_sleep_during_flash():
                ui.info(get_string("device_step2_flash"))
                port = self._ensure_edl_port(port)

                cmd = self._base_cmd(port, loader_path)
                if reset_after:
                    cmd.extend(["--reset-mode", "system"])
                cmd.append("flasher")
                for xml_path in program_xmls:
                    cmd.extend(["-p", str(xml_path)])
                for xml_path in patch_xmls:
                    cmd.extend(["-x", str(xml_path)])
                self._run_command(cmd)
        except (
            subprocess.CalledProcessError,
            OSError,
            RuntimeError,
            DeviceCommandError,
        ) as e:
            raise DeviceCommandError(
                get_string("device_err_rawprogram_fail").format(e=e),
                e,
            )
