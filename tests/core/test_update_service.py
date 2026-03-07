from unittest.mock import patch

import pytest

from ltbox import update_service


def test_read_current_version_invalid_json_returns_default(tmp_path, monkeypatch):
    bad_config = tmp_path / "config.json"
    bad_config.write_text("{bad-json", encoding="utf-8")

    monkeypatch.setattr(update_service, "APP_DIR", tmp_path)
    assert update_service.read_current_version() == "v0.0.0"


def test_get_latest_version_prefers_release():
    with patch(
        "ltbox.update_service.utils.get_latest_release_versions",
        return_value=("v2.0.0", "v2.1.0-beta"),
    ):
        latest, release, prerelease = update_service.get_latest_version("v1.0.0")

    assert latest == "v2.0.0"
    assert release == "v2.0.0"
    assert prerelease == "v2.1.0-beta"


def test_prompt_for_update_accept_yes_exits(monkeypatch):
    monkeypatch.setattr("builtins.input", lambda *_: "y")

    with (
        patch("ltbox.update_service.ui.echo"),
        patch("ltbox.update_service.webbrowser.open") as mock_open,
        pytest.raises(SystemExit),
    ):
        update_service.prompt_for_update("v1.0.0", "v1.1.0")

    mock_open.assert_called_once()


def test_prompt_for_update_accept_no_returns_false(monkeypatch):
    monkeypatch.setattr("builtins.input", lambda *_: "n")

    with (
        patch("ltbox.update_service.ui.echo"),
        patch("ltbox.update_service.ui.clear") as mock_clear,
    ):
        result = update_service.prompt_for_update("v1.0.0", "v1.1.0")

    assert result is False
    mock_clear.assert_called_once()
