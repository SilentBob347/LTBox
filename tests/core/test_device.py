from unittest.mock import MagicMock, patch

import pytest
from ltbox.device import AdbManager, EdlManager


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


def test_edl_flash_rawprogram_sends_pre_erase_and_inline_reset(tmp_path):
    manager = EdlManager()
    loader_path = tmp_path / "xbl_s_devprg_ns.melf"
    raw_xml = tmp_path / "rawprogram1.xml"
    patch_xml = tmp_path / "patch0.xml"
    fh_loader = tmp_path / "fh_loader.exe"
    qsahara = tmp_path / "QSaharaServer.exe"

    for path in (loader_path, raw_xml, patch_xml, fh_loader, qsahara):
        path.write_text("x", encoding="utf-8")

    with (
        patch("ltbox.device.const.EDL_EXE", fh_loader),
        patch("ltbox.device.const.QSAHARASERVER_EXE", qsahara),
        patch.object(manager, "load_programmer_safe"),
        patch("ltbox.device.utils.run_command") as mock_run_command,
    ):
        manager.flash_rawprogram(
            "COM1",
            loader_path,
            "UFS",
            [raw_xml],
            [patch_xml],
            pre_erase=True,
            reset_after=True,
        )

    erase_cmd = mock_run_command.call_args_list[0].args[0]
    flash_cmd = mock_run_command.call_args_list[1].args[0]

    assert "--sendxml=FHLoaderErase.xml" in erase_cmd
    assert "--reset" in flash_cmd


def test_edl_flash_rawprogram_skips_pre_erase_and_inline_reset_when_disabled(
    tmp_path,
):
    manager = EdlManager()
    loader_path = tmp_path / "xbl_s_devprg_ns.melf"
    raw_xml = tmp_path / "rawprogram1.xml"
    patch_xml = tmp_path / "patch0.xml"
    fh_loader = tmp_path / "fh_loader.exe"
    qsahara = tmp_path / "QSaharaServer.exe"

    for path in (loader_path, raw_xml, patch_xml, fh_loader, qsahara):
        path.write_text("x", encoding="utf-8")

    with (
        patch("ltbox.device.const.EDL_EXE", fh_loader),
        patch("ltbox.device.const.QSAHARASERVER_EXE", qsahara),
        patch.object(manager, "load_programmer_safe"),
        patch("ltbox.device.utils.run_command") as mock_run_command,
    ):
        manager.flash_rawprogram(
            "COM1",
            loader_path,
            "UFS",
            [raw_xml],
            [patch_xml],
            pre_erase=False,
            reset_after=False,
        )

    flash_cmd = mock_run_command.call_args_list[0].args[0]

    assert mock_run_command.call_count == 1
    assert "--sendxml=FHLoaderErase.xml" not in flash_cmd
    assert "--reset" not in flash_cmd
