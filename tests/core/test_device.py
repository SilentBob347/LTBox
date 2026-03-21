from unittest.mock import MagicMock, patch

import pytest
from ltbox.device import AdbManager


def test_adb_get_model_retry_success():
    manager = AdbManager(skip_adb=False)

    mock_device = MagicMock()
    mock_device.get_state.return_value = "device"
    mock_device.prop.model = "Lenovo TB-Test"

    with (
        patch(
            "adbutils.adb.device_list",
            side_effect=[[], [], [mock_device], [mock_device]],
        ),
        patch("ltbox.device.time.sleep", return_value=None),
    ):
        model = manager.get_model()
        assert model == "Lenovo TB-Test"


def test_fastboot_slot_detection_failure():
    import subprocess

    from ltbox.device import DeviceCommandError, FastbootManager

    manager = FastbootManager()

    with patch(
        "ltbox.utils.run_command", side_effect=subprocess.CalledProcessError(1, "cmd")
    ):
        with pytest.raises(DeviceCommandError):
            manager.get_slot_suffix()


def test_adb_reboot_edl_does_not_force_kill_processes():
    manager = AdbManager(skip_adb=False)

    with (
        patch.object(manager, "wait_for_device", return_value=True),
        patch.object(manager, "_with_device", return_value=None),
        patch.object(manager, "_force_kill_processes") as kill_processes,
    ):
        manager.reboot("edl")

    kill_processes.assert_not_called()


def test_adb_reboot_non_edl_does_not_kill_edl_related_processes():
    manager = AdbManager(skip_adb=False)

    with (
        patch.object(manager, "wait_for_device", return_value=True),
        patch.object(manager, "_with_device", return_value=None),
        patch.object(manager, "_force_kill_processes") as kill_processes,
    ):
        manager.reboot("bootloader")

    kill_processes.assert_not_called()
