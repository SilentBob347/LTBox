import subprocess
from contextlib import nullcontext
from unittest.mock import patch

import pytest

from ltbox.registry import CommandRegistry
from ltbox.errors import ToolError
from ltbox.task_runner import TaskUIAdapter, _build_final_kwargs, run_task
from tests.helpers import make_device_mock


def test_run_task_raises_for_unknown_command():
    with pytest.raises(ToolError):
        run_task("unknown", None, CommandRegistry())


def test_run_task_handles_called_process_error_and_cleans_up():
    registry = CommandRegistry()

    def failing_cmd(dev):
        raise subprocess.CalledProcessError(
            returncode=1,
            cmd=["fake", "cmd"],
            output="stdout-data",
            stderr="stderr-data",
        )

    registry.add("fail", failing_cmd, "Fail Task")
    dev = make_device_mock()

    with (
        patch("ltbox.task_runner.logging_context", return_value=nullcontext()),
        patch("ltbox.task_runner.ui.box_output") as mock_box,
        patch("builtins.input", return_value=""),
    ):
        run_task("fail", dev, registry)

    assert mock_box.called
    dev.adb.force_kill_server.assert_called_once()
    dev.fastboot.force_kill_server.assert_called_once()


def test_run_task_unhandled_exception_bubbles_up():
    registry = CommandRegistry()

    def crash_cmd(dev):
        raise ZeroDivisionError("boom")

    registry.add("crash", crash_cmd, "Crash Task")
    dev = make_device_mock()

    with (
        patch("ltbox.task_runner.logging_context", return_value=nullcontext()),
        patch("builtins.input", return_value=""),
    ):
        with pytest.raises(ZeroDivisionError):
            run_task("crash", dev, registry)

    dev.adb.force_kill_server.assert_called_once()
    dev.fastboot.force_kill_server.assert_called_once()


def test_build_final_kwargs_merges_extra_and_dev_injection():
    kwargs = _build_final_kwargs(
        base_kwargs={"a": 1},
        extra_kwargs={"b": 2},
        require_dev=True,
        dev="dev-obj",
    )

    assert kwargs == {"a": 1, "b": 2, "dev": "dev-obj"}


def test_run_task_accepts_custom_ui_adapter_without_input_patch():
    registry = CommandRegistry()
    registry.add("ok", lambda: "done", "OK Task", require_dev=False)

    events = []
    test_ui = TaskUIAdapter(
        clear=lambda: events.append("clear"),
        info=lambda msg: events.append(("info", msg)),
        echo=lambda msg: events.append(("echo", msg)),
        error=lambda msg: events.append(("error", msg)),
        box_output=lambda lines: events.append(("box", tuple(lines))),
        pause=lambda: events.append("pause"),
    )

    with patch("ltbox.task_runner.logging_context", return_value=nullcontext()):
        run_task("ok", None, registry, ui_adapter=test_ui)

    assert "clear" in events
    assert ("echo", "done") in events
    assert "pause" in events
