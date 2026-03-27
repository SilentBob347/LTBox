from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Callable, Optional

from .i18n import get_string


@dataclass(frozen=True)
class TaskResult:
    messages: list[str] = field(default_factory=list)

    @classmethod
    def from_message(cls, message: Optional[str]) -> "TaskResult":
        if not message:
            return cls()
        return cls(messages=[message])


def build_log_filename(base_dir: Path, filename_prefix: str) -> str:
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_dir = base_dir / "log"
    log_dir.mkdir(parents=True, exist_ok=True)
    return str(log_dir / f"{filename_prefix}_{timestamp}.txt")


def announce_logging_start(
    *, command_name: str, log_file: str, info: Callable[[str], None]
) -> None:
    info(get_string("logging_enabled").format(log_file=log_file))
    info(get_string("logging_command").format(command=command_name))


def announce_logging_finished(*, log_file: str, info: Callable[[str], None]) -> None:
    info(get_string("logging_finished").format(log_file=log_file))


def emit_task_result(result: Any, echo: Callable[[str], None]) -> None:
    if isinstance(result, TaskResult):
        for message in result.messages:
            if message:
                echo(message)
        return

    if isinstance(result, str):
        if result:
            echo(result)
        return

    if result:
        echo(get_string("act_unhandled_success_result").format(res=result))
