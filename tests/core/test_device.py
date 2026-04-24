import xml.etree.ElementTree as ET
from unittest.mock import MagicMock, patch

import pytest
from ltbox.device import AdbManager, DeviceController, EdlManager, FastbootManager


def test_adb_get_model_retry_success():
    manager = AdbManager(skip_adb=False)
    mock_device = MagicMock()
    mock_device.get_state.return_value = "device"
    mock_device.prop.model = "Lenovo TB-Test"

    with (
        patch.object(
            manager._client,
            "device_list",
            side_effect=[[], [], [mock_device], [mock_device]],
        ),
        patch.object(manager._client, "device", return_value=mock_device),
        patch("ltbox.utils.time.sleep", return_value=None),
    ):
        model = manager.get_model()
        assert model == "Lenovo TB-Test"


def test_fastboot_slot_detection_failure():
    import subprocess

    from ltbox.device import DeviceCommandError, FastbootManager

    manager = FastbootManager()

    with patch(
        "ltbox.device.fastboot.DeviceCommandRunner.run",
        side_effect=subprocess.CalledProcessError(1, "cmd"),
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


def test_edl_flash_rawprogram_sends_pre_erase_and_reset(tmp_path):
    manager = EdlManager()
    loader_path = tmp_path / "xbl_s_devprg_ns.melf"
    raw_xml = tmp_path / "rawprogram1.xml"
    patch_xml = tmp_path / "patch0.xml"
    qdlrs = tmp_path / "qdl-rs.exe"

    for path in (loader_path, patch_xml, qdlrs):
        path.write_text("x", encoding="utf-8")

    raw_xml.write_text(
        """<?xml version="1.0"?>
<data>
  <program label="metadata" physical_partition_number="0" filename="metadata.img"
           start_sector="100" num_partition_sectors="2"
           SECTOR_SIZE_IN_BYTES="4096" />
  <program label="frp" physical_partition_number="0" filename=""
           start_sector="200" num_partition_sectors="128"
           start_byte_hex="0x16108000" SECTOR_SIZE_IN_BYTES="4096" />
  <program label="userdata_a" physical_partition_number="6" filename="userdata.img"
           start_sector="4096" num_partition_sectors="8192"
           SECTOR_SIZE_IN_BYTES="4096" />
  <program label="super" physical_partition_number="0" filename="super.img"
           start_sector="9999" num_partition_sectors="32"
           SECTOR_SIZE_IN_BYTES="4096" />
</data>
""",
        encoding="utf-8",
    )

    with (
        patch("ltbox.device.edl.const.QDLRS_EXE", qdlrs),
        patch.object(manager, "load_programmer_safe"),
        patch.object(manager, "_ensure_edl_port", side_effect=lambda p, **kw: p),
        patch.object(manager, "_run_command") as mock_run,
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

    assert mock_run.call_count == 1
    flash_cmd = mock_run.call_args_list[0].args[0]
    assert "flasher" in flash_cmd
    assert "-p" in flash_cmd
    assert "-x" in flash_cmd

    erase_xml = tmp_path / "FHLoaderErase.xml"
    p_indices = [i for i, item in enumerate(flash_cmd) if item == "-p"]
    assert flash_cmd[p_indices[0] + 1] == str(erase_xml)
    assert flash_cmd[p_indices[1] + 1] == str(raw_xml)

    erase_entries = ET.parse(erase_xml).getroot().findall("erase")
    assert [entry.get("label") for entry in erase_entries] == [
        "metadata",
        "frp",
        "userdata_a",
    ]
    assert all(entry.get("filename") is None for entry in erase_entries)

    # reset_after embeds --reset-mode system in the flasher command
    rm_idx = flash_cmd.index("--reset-mode")
    assert flash_cmd[rm_idx + 1] == "system"


def test_edl_flash_rawprogram_requires_erase_spans_for_pre_erase(tmp_path):
    from ltbox.device import DeviceCommandError

    manager = EdlManager()
    loader_path = tmp_path / "xbl_s_devprg_ns.melf"
    raw_xml = tmp_path / "rawprogram1.xml"
    patch_xml = tmp_path / "patch0.xml"
    qdlrs = tmp_path / "qdl-rs.exe"

    for path in (loader_path, patch_xml, qdlrs):
        path.write_text("x", encoding="utf-8")

    raw_xml.write_text(
        '<data><program label="super" physical_partition_number="0" '
        'start_sector="1" num_partition_sectors="2"/></data>',
        encoding="utf-8",
    )

    with (
        patch("ltbox.device.edl.const.QDLRS_EXE", qdlrs),
        patch.object(manager, "load_programmer_safe"),
    ):
        with pytest.raises(DeviceCommandError, match="erase spans"):
            manager.flash_rawprogram(
                "COM1",
                loader_path,
                "UFS",
                [raw_xml],
                [patch_xml],
                pre_erase=True,
                reset_after=False,
            )


def test_edl_flash_rawprogram_skips_erase_and_reset_when_disabled(tmp_path):
    manager = EdlManager()
    loader_path = tmp_path / "xbl_s_devprg_ns.melf"
    raw_xml = tmp_path / "rawprogram1.xml"
    patch_xml = tmp_path / "patch0.xml"
    qdlrs = tmp_path / "qdl-rs.exe"

    for path in (loader_path, raw_xml, patch_xml, qdlrs):
        path.write_text("x", encoding="utf-8")

    with (
        patch("ltbox.device.edl.const.QDLRS_EXE", qdlrs),
        patch.object(manager, "load_programmer_safe"),
        patch.object(manager, "_ensure_edl_port", side_effect=lambda p, **kw: p),
        patch.object(manager, "_run_command") as mock_run,
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

    # Only the flasher command
    assert mock_run.call_count == 1
    flash_cmd = mock_run.call_args_list[0].args[0]
    assert "flasher" in flash_cmd
    assert "erase" not in flash_cmd


def test_edl_write_partition_leaves_success_logging_to_caller(tmp_path):
    manager = EdlManager()
    image_path = tmp_path / "init_boot.img"
    qdlrs = tmp_path / "qdl-rs.exe"
    image_path.write_text("patched", encoding="utf-8")
    qdlrs.write_text("x", encoding="utf-8")

    with (
        patch("ltbox.device.edl.const.QDLRS_EXE", qdlrs),
        patch("ltbox.device.edl.const.CONF") as mock_conf,
        patch.object(manager, "_ensure_edl_port", return_value="COM5"),
        patch.object(manager, "_run_command"),
        patch("ltbox.device.edl.ui") as mock_ui,
    ):
        mock_conf.edl_loader_file = tmp_path / "loader.melf"
        manager.write_partition(
            port="COM5",
            image_path=image_path,
            lun="4",
            start_sector="205962",
        )

    mock_ui.info.assert_not_called()


def test_fastboot_wait_for_device_uses_transient_status():
    manager = FastbootManager()
    status_cm = MagicMock()
    status_cm.__enter__.return_value = None
    status_cm.__exit__.return_value = False
    strings = {
        "device_wait_mode_title": "WAIT {mode}",
        "device_wait_fastboot_loop": "Waiting for fastboot...",
        "device_fastboot_connected": "[+] Fastboot connected.",
        "device_wait_fastboot_cancel": "[!] Cancelled.",
    }

    with (
        patch.object(manager, "_usb_port_hint"),
        patch.object(manager, "check_device", side_effect=[False, True]),
        patch("ltbox.device.fastboot.get_string", side_effect=strings.__getitem__),
        patch("ltbox.device.fastboot.ui") as mock_ui,
        patch("ltbox.device.fastboot.utils.wait_for_condition") as mock_wait,
    ):
        mock_ui.status.return_value = status_cm
        mock_wait.side_effect = (
            lambda predicate, interval=1.0, timeout=None, on_loop=None: predicate()
        )

        assert manager.wait_for_device() is True

    mock_ui.status.assert_called_once_with(strings["device_wait_fastboot_loop"])
    assert mock_wait.call_args.kwargs.get("on_loop") is None


def test_edl_reset_to_edl_calls_reset_with_edl_mode(tmp_path):
    manager = EdlManager()
    qdlrs = tmp_path / "qdl-rs.exe"
    qdlrs.write_text("x", encoding="utf-8")

    with (
        patch("ltbox.device.edl.const.QDLRS_EXE", qdlrs),
        patch("ltbox.device.edl.const.CONF") as mock_conf,
        patch.object(manager, "_ensure_edl_port", return_value="COM3"),
        patch.object(manager, "_run_command") as mock_run,
    ):
        mock_conf.edl_loader_file = tmp_path / "loader.melf"
        manager.reset_to_edl("COM3")

    cmd = mock_run.call_args.args[0]
    assert cmd[-2:] == ["reset", "edl"]
    assert "--reset-mode" in cmd
    rm_idx = cmd.index("--reset-mode")
    assert cmd[rm_idx + 1] == "edl"


def _mock_clock(step: float = 0.5):
    return [i * step for i in range(200)]


def test_ensure_edl_port_ignores_stale_port_until_reconnect_finishes():
    manager = EdlManager()

    with (
        patch("ltbox.device.edl.time.sleep", return_value=None),
        patch("ltbox.device.edl.time.monotonic", side_effect=_mock_clock()),
        patch(
            "ltbox.device.edl.find_edl_port",
            side_effect=["COM6", None, "COM7", "COM7"],
        ),
    ):
        assert manager._ensure_edl_port("COM6", timeout=30.0) == "COM7"


def test_ensure_edl_port_returns_visible_port_when_no_disconnect_happens():
    manager = EdlManager()

    with (
        patch("ltbox.device.edl.time.sleep", return_value=None),
        patch("ltbox.device.edl.time.monotonic", side_effect=_mock_clock()),
        patch("ltbox.device.edl.find_edl_port", side_effect=["COM6"] * 30),
    ):
        assert manager._ensure_edl_port("COM6", timeout=30.0) == "COM6"


def test_ensure_edl_port_raises_when_port_never_returns():
    from ltbox.device import DeviceCommandError

    manager = EdlManager()

    with (
        patch("ltbox.device.edl.time.sleep", return_value=None),
        patch("ltbox.device.edl.time.monotonic", side_effect=_mock_clock(step=1.0)),
        patch("ltbox.device.edl.find_edl_port", return_value=None),
    ):
        with pytest.raises(DeviceCommandError):
            manager._ensure_edl_port("COM6", timeout=10.0)


def test_ensure_edl_mode_prefers_adb_reboot_from_fastboot():
    adb = MagicMock(skip_adb=False)
    fastboot = MagicMock()
    edl = MagicMock()
    controller = DeviceController(
        adb_manager=adb,
        fastboot_manager=fastboot,
        edl_manager=edl,
    )
    edl.check_device.return_value = False
    fastboot.check_device.return_value = True

    with patch("ltbox.device.controller.ui"):
        controller.ensure_edl_mode()

    fastboot.oem_edl.assert_not_called()
    fastboot.continue_boot.assert_called_once()
    adb.wait_for_device.assert_called_once()
    adb.reboot.assert_called_once_with("edl")


def test_setup_edl_connection_waits_for_manual_edl_when_skip_adb_from_fastboot(
    mock_env,
):
    adb = MagicMock(skip_adb=True)
    fastboot = MagicMock()
    edl = MagicMock()
    controller = DeviceController(
        adb_manager=adb,
        fastboot_manager=fastboot,
        edl_manager=edl,
    )
    edl.check_device.return_value = False
    edl.wait_for_device.return_value = "COM9"
    fastboot.check_device.return_value = True

    with patch("ltbox.device.controller.ui") as mock_ui:
        port = controller.setup_edl_connection()

    assert port == "COM9"
    fastboot.oem_edl.assert_not_called()
    fastboot.continue_boot.assert_not_called()
    adb.wait_for_device.assert_not_called()
    adb.reboot.assert_not_called()
    edl.wait_for_device.assert_called_once()
    assert mock_ui.echo.call_count >= 3


def test_edl_session_logs_single_reset_message():
    controller = DeviceController(
        adb_manager=MagicMock(skip_adb=False),
        fastboot_manager=MagicMock(),
        edl_manager=MagicMock(),
    )

    messages = {
        "act_reset_sys": "[*] Rebooting to System...",
        "act_reset_sent": "[+] Reboot command sent.",
    }

    with (
        patch.object(controller, "setup_edl_connection", return_value="COM9"),
        patch("ltbox.device.controller.get_string", side_effect=messages.__getitem__),
        patch("ltbox.device.controller.ui") as mock_ui,
    ):
        with controller.edl_session(
            load_programmer=False, reset_msg_key="act_reset_sys"
        ):
            pass

    echoed_messages = [call.args[0] for call in mock_ui.info.call_args_list]
    assert echoed_messages == [
        messages["act_reset_sys"],
        messages["act_reset_sent"],
    ]


def test_edl_base_cmd_uses_qdlrs_serial_backend(tmp_path):
    manager = EdlManager()
    loader = tmp_path / "loader.melf"

    cmd = manager._base_cmd("COM12", loader)
    assert "--backend" in cmd
    assert "serial" in cmd
    assert "-d" in cmd
    assert "COM12" in cmd
    assert "-s" in cmd
    assert "ufs" in cmd
    assert "--reset-mode" not in cmd
