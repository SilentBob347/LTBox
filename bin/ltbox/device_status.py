import threading
from typing import Callable, Dict, Optional

from . import constants as const
from .device_support import DeviceCommandRunner, is_qualcomm_edl_port
from .i18n import get_string


class DeviceStatusMonitor:
    """Polls device connection status in a background thread."""

    def __init__(self, interval: float = 3.0, idle_interval: Optional[float] = None):
        self._interval = interval
        self._idle_interval = idle_interval or max(interval, 5.0)
        self._status_key = "device_status_unknown"
        self._lock = threading.Lock()
        self._stop_event = threading.Event()
        self._thread: Optional[threading.Thread] = None
        self._command_runner = DeviceCommandRunner()

    def get_status_key(self) -> str:
        with self._lock:
            return self._status_key

    def get_status_text(self) -> str:
        with self._lock:
            return get_string(self._status_key)

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        try:
            self._status_key = self._detect()
        except Exception:
            pass
        self._stop_event.clear()
        self._thread = threading.Thread(target=self._poll_loop, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop_event.set()
        if self._thread:
            self._thread.join(timeout=5)
            self._thread = None

    def _detect_serial_status(self) -> Optional[str]:
        try:
            import serial.tools.list_ports

            for port in serial.tools.list_ports.comports():
                hwid = (port.hwid or "").upper()
                if is_qualcomm_edl_port(port):
                    return "device_status_edl"
                if "VID:PID=05C6:900E" in hwid:
                    return "device_status_diag"
        except Exception:
            pass
        return None

    def _detect_fastboot_status(self) -> Optional[str]:
        try:
            result = self._command_runner.run(
                [str(const.FASTBOOT_EXE), "devices"],
                capture=True,
                timeout=5,
            )
            if result.stdout.strip():
                return "device_status_fastboot"
        except Exception:
            pass
        return None

    def _detect_adb_status(self) -> Optional[str]:
        try:
            result = self._command_runner.run(
                [str(const.ADB_EXE), "devices"],
                capture=True,
                check=False,
                timeout=5,
            )
            output = result.stdout.strip()
            for line in output.splitlines()[1:]:
                if "\tdevice" in line:
                    return "device_status_adb"
                if line.strip():
                    return "device_status_adb_required"
        except Exception:
            pass
        return None

    def _detector_order(self, current_key: str) -> tuple[str, ...]:
        preferred = {
            "device_status_adb": ("adb", "serial", "fastboot"),
            "device_status_adb_required": ("adb", "serial", "fastboot"),
            "device_status_fastboot": ("fastboot", "serial", "adb"),
            "device_status_edl": ("serial", "fastboot", "adb"),
            "device_status_diag": ("serial", "fastboot", "adb"),
        }
        return preferred.get(current_key, ("serial", "fastboot", "adb"))

    def _poll_interval_for(self, status_key: str) -> float:
        if status_key == "device_status_unknown":
            return self._idle_interval
        return self._interval

    def _detect(self) -> str:
        detectors: Dict[str, Callable[[], Optional[str]]] = {
            "serial": self._detect_serial_status,
            "fastboot": self._detect_fastboot_status,
            "adb": self._detect_adb_status,
        }

        with self._lock:
            current_key = self._status_key

        for detector_name in self._detector_order(current_key):
            detected = detectors[detector_name]()
            if detected:
                return detected

        return "device_status_unknown"

    def _poll_loop(self) -> None:
        while not self._stop_event.is_set():
            try:
                new_key = self._detect()
                with self._lock:
                    self._status_key = new_key
            except Exception:
                new_key = "device_status_unknown"
            self._stop_event.wait(self._poll_interval_for(new_key))
