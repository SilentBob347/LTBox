from unittest.mock import MagicMock

from ltbox.device.status import DeviceStatusMonitor


def test_device_status_prefers_active_transport_probe():
    monitor = DeviceStatusMonitor()
    monitor._status_key = "device_status_fastboot"
    monitor._detect_fastboot_status = MagicMock(return_value="device_status_fastboot")
    monitor._detect_serial_status = MagicMock(return_value=None)
    monitor._detect_adb_status = MagicMock(return_value=None)

    detected = monitor._detect()

    assert detected == "device_status_fastboot"
    monitor._detect_fastboot_status.assert_called_once_with()
    monitor._detect_serial_status.assert_not_called()
    monitor._detect_adb_status.assert_not_called()


def test_device_status_falls_back_when_active_transport_probe_misses():
    monitor = DeviceStatusMonitor()
    monitor._status_key = "device_status_fastboot"
    monitor._detect_fastboot_status = MagicMock(return_value=None)
    monitor._detect_serial_status = MagicMock(return_value="device_status_edl")
    monitor._detect_adb_status = MagicMock(return_value=None)

    detected = monitor._detect()

    assert detected == "device_status_edl"
    monitor._detect_fastboot_status.assert_called_once_with()
    monitor._detect_serial_status.assert_called_once_with()
    monitor._detect_adb_status.assert_not_called()


def test_device_status_uses_longer_idle_poll_interval_for_unknown_state():
    monitor = DeviceStatusMonitor(interval=3.0)

    assert monitor._poll_interval_for("device_status_unknown") == 5.0
    assert monitor._poll_interval_for("device_status_adb") == 3.0
