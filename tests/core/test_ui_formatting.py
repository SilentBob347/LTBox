from ltbox.logger import RichConsoleHandler
from ltbox.ui import _normalize_message


def test_normalize_message_preserves_indentation_for_ascii_art_and_breadcrumbs():
    message = (
        "\n  Main > Root > APatch\n"
        "      _             (done)\n"
        "     | |\n"
        "   __| | ___  _ __   ___\n"
    )

    normalized = _normalize_message(message)

    assert (
        normalized == "\n  Main > Root > APatch\n"
        "      _             (done)\n"
        "     | |\n"
        "   __| | ___  _ __   ___\n"
    )


def test_normalize_message_collapses_extra_space_after_status_prefix():
    message = (
        "\n[*]  매니저 APK 다운로드 중...\n"
        "[+]  매니저 APK 다운로드됨.\n"
        "  [!]  수동 확인 필요\n"
    )

    normalized = _normalize_message(message)

    assert normalized == (
        "\n[*] 매니저 APK 다운로드 중...\n"
        "[+] 매니저 APK 다운로드됨.\n"
        "  [!] 수동 확인 필요\n"
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
