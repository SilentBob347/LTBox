from ltbox.logger import RichConsoleHandler
from ltbox.ui import _normalize_message


def test_normalize_message_trims_indent_and_reflows_wrapped_lines():
    message = (
        "\n  [NOTICE] You must manually flash the generated\n"
        "       images to your device using Fastboot.\n"
    )

    normalized = _normalize_message(message)

    assert (
        normalized
        == "\n[NOTICE] You must manually flash the generated images to your device using Fastboot.\n"
    )


def test_normalize_message_keeps_header_blocks_but_removes_padding():
    message = (
        "[!] No backup found.\n\n"
        "  [LKM Unroot]\n"
        "  Place init_boot.img + vbmeta.img\n"
        "  in 'lkm'.\n\n"
        "  [GKI Unroot]\n"
        "  Place boot.img + vbmeta.img in 'gki'."
    )

    normalized = _normalize_message(message)

    assert normalized == (
        "[!] No backup found.\n\n"
        "[LKM Unroot]\n"
        "Place init_boot.img + vbmeta.img in 'lkm'.\n\n"
        "[GKI Unroot]\n"
        "Place boot.img + vbmeta.img in 'gki'."
    )


def test_logger_detects_notice_and_step_styles():
    info_record = type("Record", (), {"levelno": 20})()

    assert (
        RichConsoleHandler._detect_style(
            "[NOTICE] Manual action required.", info_record
        )
        == "yellow"
    )
    assert (
        RichConsoleHandler._detect_style("[1/6단계] 패치 준비", info_record) == "cyan"
    )
