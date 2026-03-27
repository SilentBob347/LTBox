import subprocess
import time
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Callable, List, Optional

import serial

from . import constants as const
from . import utils
from .device_support import (
    BaseDeviceManager,
    DeviceCommandRunner,
    find_edl_port,
    format_serial_port,
    prevent_sleep_during_flash,
)
from .errors import DeviceCommandError
from .i18n import get_string
from .ui import ui


class EdlManager(BaseDeviceManager):
    _FHLOADER_ERASE_FILENAME = "FHLoaderErase.xml"
    _FHLOADER_ERASE_LABELS = frozenset({"userdata", "metadata", "frp"})

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
        ui.info(get_string("device_wait_mode_title").format(mode="EDL"))
        port_name = self.check_device()
        if port_name:
            return port_name

        def _loop_msg() -> None:
            ui.info(get_string("device_wait_edl_loop"))

        try:
            port_name = utils.wait_for_condition(
                lambda: self.check_device(silent=True),
                interval=2.0,
                on_loop=_loop_msg,
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

    def load_programmer(self, port: str, loader_path: Path) -> None:
        if not const.QSAHARASERVER_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_qsahara_missing").format(
                    path=const.QSAHARASERVER_EXE
                )
            )

        cmd_sahara = [
            str(const.QSAHARASERVER_EXE),
            "-p",
            format_serial_port(port),
            "-s",
            f"13:{loader_path}",
        ]

        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd_sahara, timeout=30.0)
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
        time.sleep(2)

    def _build_fhloader_erase_entries(self, raw_xmls: List[Path]) -> List[ET.Element]:
        erase_entries: List[ET.Element] = []
        seen_entries: set[tuple[tuple[str, str], ...]] = set()

        for xml_path in raw_xmls:
            try:
                tree = ET.parse(xml_path)
            except (ET.ParseError, OSError) as e:
                raise RuntimeError(
                    f"Failed to parse '{xml_path.name}' while building "
                    f"{self._FHLOADER_ERASE_FILENAME}: {e}"
                ) from e

            for program in tree.getroot().findall("program"):
                if (
                    program.get("label", "").strip().lower()
                    not in self._FHLOADER_ERASE_LABELS
                ):
                    continue

                physical_partition_number = program.get(
                    "physical_partition_number", ""
                ).strip()
                start_sector = program.get("start_sector", "").strip()
                num_partition_sectors = program.get("num_partition_sectors", "").strip()

                if not (
                    physical_partition_number and start_sector and num_partition_sectors
                ):
                    raise RuntimeError(
                        f"'{program.get('label', 'unknown')}' entry in '{xml_path.name}' "
                        f"is missing erase geometry required to build "
                        f"{self._FHLOADER_ERASE_FILENAME}."
                    )

                erase_attrib_items = tuple(
                    (key, value)
                    for key, value in program.attrib.items()
                    if key != "filename"
                )
                if not erase_attrib_items or erase_attrib_items in seen_entries:
                    continue

                seen_entries.add(erase_attrib_items)
                erase = ET.Element("erase")
                for key, value in program.attrib.items():
                    if key != "filename":
                        erase.set(key, value)
                erase_entries.append(erase)

        if erase_entries:
            return erase_entries

        raise FileNotFoundError(
            f"Missing userdata/metadata/frp erase spans required to build "
            f"{self._FHLOADER_ERASE_FILENAME}."
        )

    def _build_fhloader_erase_xml(self, work_dir: Path, raw_xmls: List[Path]) -> Path:
        erase_root = ET.Element("data")
        for erase_entry in self._build_fhloader_erase_entries(raw_xmls):
            erase_root.append(erase_entry)

        erase_xml = work_dir / self._FHLOADER_ERASE_FILENAME
        erase_tree = ET.ElementTree(erase_root)
        ET.indent(erase_tree, space="  ")
        erase_tree.write(erase_xml, encoding="utf-8", xml_declaration=True)
        return erase_xml

    def read_partition(
        self,
        port: str,
        output_filename: str,
        lun: str,
        start_sector: str,
        num_sectors: str,
        memory_name: str = "UFS",
    ) -> None:
        if not const.EDL_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_fh_missing").format(path=const.EDL_EXE)
            )

        dest_file = Path(output_filename).resolve()
        dest_dir = dest_file.parent
        dest_filename = dest_file.name
        dest_dir.mkdir(parents=True, exist_ok=True)

        cmd_fh = [
            str(const.EDL_EXE),
            f"--port={format_serial_port(port)}",
            "--convertprogram2read",
            f"--sendimage={dest_filename}",
            f"--lun={lun}",
            f"--start_sector={start_sector}",
            f"--num_sectors={num_sectors}",
            f"--memoryname={memory_name}",
            "--noprompt",
            "--zlpawarehost=1",
        ]

        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd_fh, cwd=dest_dir)
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(get_string("device_err_fh_exec").format(e=e), e)

    def write_partition(
        self,
        port: str,
        image_path: Path,
        lun: str,
        start_sector: str,
        memory_name: str = "UFS",
    ) -> None:
        if not const.EDL_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_fh_missing").format(path=const.EDL_EXE)
            )

        image_file = Path(image_path).resolve()
        work_dir = image_file.parent
        filename = image_file.name

        cmd_fh = [
            str(const.EDL_EXE),
            f"--port={format_serial_port(port)}",
            f"--sendimage={filename}",
            f"--lun={lun}",
            f"--start_sector={start_sector}",
            f"--memoryname={memory_name}",
            "--noprompt",
            "--zlpawarehost=1",
        ]

        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd_fh, cwd=work_dir)
            ui.info(get_string("device_flash_success").format(filename=filename))
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(get_string("device_err_flash_exec").format(e=e), e)

    def reset(self, port: str) -> None:
        if not const.EDL_EXE.exists():
            raise FileNotFoundError(
                get_string("device_err_fh_missing").format(path=const.EDL_EXE)
            )

        cmd_fh = [
            str(const.EDL_EXE),
            f"--port={format_serial_port(port)}",
            "--reset",
            "--noprompt",
        ]
        try:
            with prevent_sleep_during_flash():
                self._run_command(cmd_fh, timeout=30.0)
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(get_string("device_err_reset_fail").format(e=e), e)

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
        if not const.QSAHARASERVER_EXE.exists() or not const.EDL_EXE.exists():
            ui.error(
                get_string("device_err_tools_missing").format(dir=const.TOOLS_DIR.name)
            )
            raise FileNotFoundError(get_string("device_err_edl_tools_missing"))

        search_path = str(loader_path.parent)
        ui.info(get_string("device_step1_load"))
        self.load_programmer_safe(port, loader_path)

        raw_xml_str = ",".join(path.name for path in raw_xmls)
        patch_xml_str = ",".join(path.name for path in patch_xmls)

        try:
            if pre_erase:
                self._build_fhloader_erase_xml(loader_path.parent, raw_xmls)

            with prevent_sleep_during_flash():
                if pre_erase:
                    cmd_erase = [
                        str(const.EDL_EXE),
                        f"--port={format_serial_port(port)}",
                        f"--search_path={search_path}",
                        f"--sendxml={self._FHLOADER_ERASE_FILENAME}",
                        f"--memoryname={memory_type}",
                        "--showpercentagecomplete",
                        "--zlpawarehost=1",
                        "--noprompt",
                    ]
                    self._run_command(cmd_erase)

                ui.info(get_string("device_step2_flash"))
                cmd_fh = [
                    str(const.EDL_EXE),
                    f"--port={format_serial_port(port)}",
                    f"--search_path={search_path}",
                    f"--sendxml={raw_xml_str}",
                    f"--sendxml={patch_xml_str}",
                    "--setactivepartition=1",
                    f"--memoryname={memory_type}",
                    "--showpercentagecomplete",
                    "--zlpawarehost=1",
                    "--noprompt",
                ]
                if reset_after:
                    cmd_fh.append("--reset")

                self._run_command(cmd_fh)
        except (subprocess.CalledProcessError, OSError, RuntimeError) as e:
            raise DeviceCommandError(
                get_string("device_err_rawprogram_fail").format(e=e),
                e,
            )
