from unittest.mock import MagicMock, patch

from ltbox.device.support import find_edl_port, format_serial_port, is_qualcomm_edl_port


def test_is_qualcomm_edl_port_matches_description_or_hwid():
    port = MagicMock(
        description="Qualcomm HS-USB QDLoader 9008", hwid="", device="COM7"
    )
    assert is_qualcomm_edl_port(port) is True

    port = MagicMock(description="", hwid="USB VID:PID=05C6:9008", device="COM8")
    assert is_qualcomm_edl_port(port) is True

    port = MagicMock(
        description="Other Device", hwid="USB VID:PID=1234:5678", device="COM9"
    )
    assert is_qualcomm_edl_port(port) is False


def test_find_edl_port_returns_first_matching_port():
    ports = [
        MagicMock(
            description="Other Device", hwid="USB VID:PID=1234:5678", device="COM3"
        ),
        MagicMock(
            description="Qualcomm HS-USB QDLoader 9008",
            hwid="USB VID:PID=05C6:9008",
            device="COM7",
        ),
        MagicMock(
            description="Qualcomm HS-USB QDLoader 9008",
            hwid="USB VID:PID=05C6:9008",
            device="COM8",
        ),
    ]

    with patch("serial.tools.list_ports.comports", return_value=ports):
        assert find_edl_port() == "COM7"


def test_format_serial_port_uses_windows_device_prefix():
    assert format_serial_port("COM12") == r"\\.\COM12"
