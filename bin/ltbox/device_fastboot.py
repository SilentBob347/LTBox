import re
import subprocess
from typing import Callable, Optional

from . import constants as const
from . import utils
from .device_support import BaseDeviceManager, DeviceCommandRunner
from .errors import DeviceCommandError
from .i18n import get_string
from .process_runner import CommandResult
from .ui import ui


class FastbootManager(BaseDeviceManager):
    def __init__(
        self,
        usb_port_hint: Optional[Callable[[], None]] = None,
        command_runner: Optional[DeviceCommandRunner] = None,
    ):
        super().__init__(usb_port_hint=usb_port_hint, command_runner=command_runner)

    def force_kill_server(self) -> None:
        self._force_kill_process("fastboot.exe")

    def _run_command(
        self,
        command: list[str],
        *,
        capture: bool = True,
        check: bool = False,
        timeout: Optional[float] = None,
    ) -> CommandResult:
        return self._command_runner.run(
            command,
            capture=capture,
            check=check,
            timeout=timeout,
        )

    def get_slot_suffix(self) -> Optional[str]:
        try:
            result = self._run_command(
                [str(const.FASTBOOT_EXE), "getvar", "current-slot"],
            )
            output = utils.format_command_output(result)

            match = re.search(r"current-slot:\s*([a-z]+)", output)
            if match:
                slot = match.group(1).strip()
                if slot in ["a", "b"]:
                    return f"_{slot}"

            ui.warn(
                get_string("device_warn_slot_fastboot").format(
                    snippet=output.splitlines()[0] if output else "None"
                )
            )
            return None
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(
                get_string("device_err_get_slot_fastboot").format(e=e),
                e,
            )

    def check_device(self, silent: bool = False) -> bool:
        if not silent:
            ui.info(get_string("device_check_fastboot"))
        try:
            result = self._run_command(
                [str(const.FASTBOOT_EXE), "devices"],
            )
            output = result.stdout.strip()

            if output:
                if not silent:
                    ui.info(get_string("device_found_fastboot").format(output=output))
                return True

            if not silent:
                ui.warn(get_string("device_no_fastboot"))
                ui.warn(get_string("device_connect_fastboot"))
            return False
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            if not silent:
                ui.error(get_string("device_err_check_fastboot").format(e=e))
            return False

    def wait_for_device(self) -> bool:
        self._usb_port_hint()
        ui.info(get_string("device_wait_mode_title").format(mode="fastboot"))
        if self.check_device(silent=True):
            ui.info(get_string("device_fastboot_connected"))
            return True

        def _loop_msg() -> None:
            ui.info(get_string("device_wait_fastboot_loop"))

        try:
            utils.wait_for_condition(
                lambda: self.check_device(silent=True),
                interval=2.0,
                on_loop=_loop_msg,
            )
            ui.info(get_string("device_fastboot_connected"))
            return True
        except KeyboardInterrupt:
            ui.warn(get_string("device_wait_fastboot_cancel"))
            raise

    def get_model(self) -> Optional[str]:
        try:
            result = self._run_command(
                [str(const.FASTBOOT_EXE), "getvar", "modelname"],
            )
            output = utils.format_command_output(result)

            match = re.search(r"modelname:\s*(.+)", output)
            if match:
                model = match.group(1).strip()
                if model:
                    return model
            return None
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(
                get_string("device_err_get_model_fastboot").format(e=e),
                e,
            )

    def continue_boot(self) -> None:
        try:
            self._run_command([str(const.FASTBOOT_EXE), "continue"])
        except (subprocess.CalledProcessError, FileNotFoundError) as e:
            raise DeviceCommandError(
                get_string("device_err_fastboot_continue").format(e=e),
                e,
            )

    def oem_edl(self) -> bool:
        try:
            result = self._run_command([str(const.FASTBOOT_EXE), "oem", "edl"])
            return "FAILED" not in utils.format_command_output(result)
        except (subprocess.CalledProcessError, FileNotFoundError):
            return False
