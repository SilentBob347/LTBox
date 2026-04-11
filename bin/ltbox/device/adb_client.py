from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .. import constants as const
from .support import DeviceCommandRunner


class AdbError(RuntimeError):
    pass


@dataclass(frozen=True)
class AdbDeviceInfo:
    serial: str
    state: str


class AdbSync:
    def __init__(self, client: "AdbClient", serial: str):
        self._client = client
        self._serial = serial

    def push(self, local: str, remote: str) -> None:
        self._client.run(
            self._serial,
            "push",
            local,
            remote,
            timeout=300,
        )

    def pull(self, remote: str, local: str) -> None:
        self._client.run(
            self._serial,
            "pull",
            remote,
            local,
            timeout=300,
        )


class AdbProps:
    def __init__(self, device: "AdbDevice"):
        self._device = device

    @property
    def model(self) -> str:
        return self._device.getprop("ro.product.model")


class AdbDevice:
    def __init__(self, client: "AdbClient", serial: str, state: str = "device"):
        self._client = client
        self._serial = serial
        self._state = state
        self.prop = AdbProps(self)
        self.sync = AdbSync(client, serial)

    def get_state(self) -> str:
        return self._state

    def getprop(self, prop: str) -> str:
        return self.shell(f"getprop {prop}").strip()

    def shell(self, cmd: str) -> str:
        return self._client.shell(self._serial, cmd)

    def reboot(self, target: str) -> None:
        self._client.reboot(self._serial, target)

    def install(self, apk_path: str) -> None:
        self._client.install(self._serial, apk_path)


class AdbClient:
    def __init__(
        self,
        command_runner: Optional[DeviceCommandRunner] = None,
        adb_exe: Optional[Path] = None,
    ):
        self._runner = command_runner or DeviceCommandRunner()
        self._adb_exe = adb_exe or const.ADB_EXE

    def _adb_command(self, *args: str) -> list[str]:
        executable = (
            str(self._adb_exe)
            if self._adb_exe.exists()
            else os.environ.get("ADB") or "adb"
        )
        return [executable, *args]

    def _run_capture(
        self,
        *args: str,
        timeout: Optional[float] = 20,
    ) -> str:
        try:
            result = self._runner.run(
                self._adb_command(*args),
                capture=True,
                timeout=timeout,
            )
            return result.stdout.strip()
        except (subprocess.CalledProcessError, OSError, subprocess.TimeoutExpired) as e:
            raise AdbError(str(e)) from e

    def run(
        self,
        serial: str,
        *args: str,
        timeout: Optional[float] = 20,
    ) -> None:
        try:
            self._runner.run(
                self._adb_command("-s", serial, *args),
                timeout=timeout,
            )
        except (subprocess.CalledProcessError, OSError, subprocess.TimeoutExpired) as e:
            raise AdbError(str(e)) from e

    def shell(
        self,
        serial: str,
        cmd: str,
        *,
        timeout: Optional[float] = 60,
    ) -> str:
        return self._run_capture("-s", serial, "shell", cmd, timeout=timeout)

    def reboot(
        self,
        serial: str,
        target: str,
        *,
        timeout: Optional[float] = 20,
    ) -> None:
        if target in ("", "system"):
            self.run(serial, "reboot", timeout=timeout)
            return
        self.run(serial, "reboot", target, timeout=timeout)

    def install(
        self,
        serial: str,
        apk_path: str,
        *,
        timeout: Optional[float] = 300,
    ) -> None:
        self.run(serial, "install", "-r", apk_path, timeout=timeout)

    @staticmethod
    def _parse_device_list(output: str) -> list[AdbDeviceInfo]:
        infos: list[AdbDeviceInfo] = []
        for line in output.splitlines():
            line = line.strip()
            if not line or line.startswith("List of devices attached"):
                continue
            parts = line.split()
            if len(parts) < 2:
                continue
            infos.append(AdbDeviceInfo(serial=parts[0], state=parts[1]))
        return infos

    def device_list(self) -> list[AdbDevice]:
        output = self._run_capture("devices")
        return [
            AdbDevice(self, serial=info.serial, state=info.state)
            for info in self._parse_device_list(output)
        ]

    def device(self, serial: Optional[str] = None) -> AdbDevice:
        devices = self.device_list()
        if serial is not None:
            for device in devices:
                if device._serial == serial:
                    return device
            raise AdbError(f"Device not found: {serial}")

        online_devices = [
            device for device in devices if device.get_state() == "device"
        ]
        if not online_devices:
            raise AdbError("Can't find any android device/emulator")
        if len(online_devices) > 1:
            raise AdbError(
                "more than one device/emulator, please specify the serial number"
            )
        return online_devices[0]
