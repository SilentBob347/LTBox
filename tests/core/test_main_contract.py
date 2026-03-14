import json
from pathlib import Path

import pytest
from ltbox import main


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
