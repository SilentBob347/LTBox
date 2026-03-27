import functools
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional

from .errors import LTBoxError, ToolError
from .execution import (
    announce_logging_finished,
    announce_logging_start,
    build_log_filename,
    emit_task_result,
)
from .i18n import get_string
from .logger import logging_context
from .registry import CommandRegistry
from .utils import ui

APP_DIR = Path(__file__).parent.resolve()
BASE_DIR = APP_DIR.parent


@dataclass(frozen=True)
class TaskUIAdapter:
    clear: Callable[[], None]
    info: Callable[[str], None]
    echo: Callable[[str], None]
    error: Callable[[str], None]
    box_output: Callable[[List[str]], None]
    pause: Callable[[], None]


def _default_ui_adapter() -> TaskUIAdapter:
    def _pause() -> None:
        input(get_string("press_enter_to_continue"))

    return TaskUIAdapter(
        clear=ui.clear,
        info=ui.info,
        echo=ui.echo,
        error=ui.error,
        box_output=lambda lines: ui.box_output(lines, err=True),
        pause=_pause,
    )


def _format_command_failure_messages(
    error: subprocess.CalledProcessError,
) -> List[str]:
    messages = [
        get_string("err_cmd_failed").format(
            cmd=" ".join(error.cmd) if isinstance(error.cmd, list) else error.cmd
        )
    ]
    if error.stdout:
        messages.append(f"{get_string('err_cmd_stdout_header')}\n{error.stdout}")
    if error.stderr:
        messages.append(f"{get_string('err_cmd_stderr_header')}\n{error.stderr}")
    return messages


@functools.singledispatch
def _handle_task_error(
    error: BaseException, title: str, ui_adapter: TaskUIAdapter
) -> None:
    pass


@_handle_task_error.register
def _(error: LTBoxError, title: str, ui_adapter: TaskUIAdapter) -> None:
    ui_adapter.box_output([get_string("task_failed").format(title=title), str(error)])


@_handle_task_error.register
def _(
    error: subprocess.CalledProcessError, title: str, ui_adapter: TaskUIAdapter
) -> None:
    ui_adapter.box_output(_format_command_failure_messages(error))


@_handle_task_error.register(FileNotFoundError)
@_handle_task_error.register(RuntimeError)
@_handle_task_error.register(KeyError)
def _(error: Exception, title: str, ui_adapter: TaskUIAdapter) -> None:
    ui_adapter.box_output([get_string("unexpected_error").format(e=error)])


@_handle_task_error.register
def _(error: SystemExit, title: str, ui_adapter: TaskUIAdapter) -> None:
    ui_adapter.error(get_string("process_halted"))


@_handle_task_error.register
def _(error: KeyboardInterrupt, title: str, ui_adapter: TaskUIAdapter) -> None:
    ui_adapter.error(get_string("process_cancelled"))


def _build_final_kwargs(
    base_kwargs: Dict[str, Any],
    extra_kwargs: Optional[Dict[str, Any]],
    require_dev: bool,
    dev: Any,
) -> Dict[str, Any]:
    final_kwargs = base_kwargs.copy()
    if extra_kwargs:
        final_kwargs.update(extra_kwargs)
    if require_dev:
        final_kwargs["dev"] = dev
    return final_kwargs


def run_task(
    command: str,
    dev: Any,
    registry: CommandRegistry,
    extra_kwargs: Optional[Dict[str, Any]] = None,
    ui_adapter: Optional[TaskUIAdapter] = None,
):
    ui_adapter = ui_adapter or _default_ui_adapter()

    ui_adapter.clear()

    cmd_info = registry.get(command)
    if not cmd_info:
        raise ToolError(get_string("unknown_command").format(command=command))

    title = cmd_info.title
    func = cmd_info.func
    base_kwargs = cmd_info.default_kwargs
    require_dev = cmd_info.require_dev
    result_handler = cmd_info.result_handler
    log_filename = build_log_filename(
        BASE_DIR.parent,
        cmd_info.log_filename_prefix or f"log_{command}",
    )

    try:
        announce_logging_start(
            command_name=command,
            log_file=log_filename,
            info=ui_adapter.info,
        )

        with logging_context(log_filename):
            if dev and hasattr(dev, "reset_task_state"):
                dev.reset_task_state()

            final_kwargs = _build_final_kwargs(
                base_kwargs=base_kwargs,
                extra_kwargs=extra_kwargs,
                require_dev=require_dev,
                dev=dev,
            )

            result = func(**final_kwargs)
            if result_handler:
                result = result_handler(result)
            emit_task_result(result, ui_adapter.echo)

    except (
        LTBoxError,
        subprocess.CalledProcessError,
        FileNotFoundError,
        RuntimeError,
        KeyError,
        OSError,
        ValueError,
        TypeError,
        SystemExit,
        KeyboardInterrupt,
    ) as e:
        _handle_task_error(e, title, ui_adapter)
    finally:
        if dev and hasattr(dev, "adb"):
            dev.adb.force_kill_server()
        if dev and hasattr(dev, "fastboot"):
            dev.fastboot.force_kill_server()

        announce_logging_finished(log_file=log_filename, info=ui_adapter.info)

        ui_adapter.echo("")
        ui_adapter.pause()
