import json
from pathlib import Path

import pytest
from ltbox import main
from ltbox import menu_router
from ltbox.app_state import AppState


def test_imports():
    assert hasattr(main, "CommandRegistry")
    assert hasattr(main, "setup_console")


def test_registry_add_and_get():
    registry = main.CommandRegistry()

    def dummy():
        return "ok"

    registry.add("cmd", dummy, "Title", require_dev=False)
    command_info = registry.get("cmd")
    assert command_info["title"] == "Title"
    assert command_info["func"]() == "ok"


def test_json_validity():
    ltbox_dir = Path(__file__).resolve().parents[2] / "bin" / "ltbox"
    files = list(ltbox_dir.rglob("*.json"))
    if not files:
        pytest.skip("No JSON")

    for path in files:
        with open(path, "r", encoding="utf-8") as fp:
            json.load(fp)


def test_config_keys():
    config_path = Path(__file__).resolve().parents[2] / "bin" / "ltbox" / "config.json"
    if config_path.exists():
        with open(config_path, "r", encoding="utf-8") as file:
            config = json.load(file)
        assert "version" in config


def test_main_loop_exits_only_at_top_level(monkeypatch):
    monkeypatch.setattr(
        menu_router,
        "_loop_menu",
        lambda *_args, **_kwargs: menu_router.LoopAction.EXIT,
    )

    with pytest.raises(SystemExit) as exc:
        menu_router.main_loop(
            device_controller_class=lambda skip_adb: type(
                "Dev", (), {"skip_adb": skip_adb}
            )(),
            registry=main.CommandRegistry(),
            initial_state=AppState(),
        )

    assert exc.value.code == 0
