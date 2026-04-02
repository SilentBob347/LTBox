import re
from contextlib import contextmanager
from typing import List

from .logger import console, get_logger

logger = get_logger()

_PREFIX_SPACING_PATTERN = re.compile(r"^(\s*\[[^\]]+\])\s{2,}(?=\S)")


def _normalize_line(line: str) -> str:
    trimmed = line.rstrip()
    return _PREFIX_SPACING_PATTERN.sub(r"\1 ", trimmed)


def _normalize_message(message: str) -> str:
    if not message:
        return message

    normalized = message.replace("\r\n", "\n")
    return "\n".join(_normalize_line(line) for line in normalized.split("\n"))


class ConsoleUI:
    def get_term_width(self, max_width: int = 78) -> int:
        return min(max_width, console.width)

    def echo(self, message: str = "", err: bool = False) -> None:
        message = _normalize_message(message)
        if err:
            logger.error(message)
        else:
            logger.info(message)

    def info(self, message: str) -> None:
        self.echo(message)

    def warn(self, message: str) -> None:
        self.echo(f"\033[93m{message}\033[0m", err=True)

    def error(self, message: str) -> None:
        self.echo(f"\033[91m{message}\033[0m", err=True)

    def box_output(self, lines: List[str], err: bool = False) -> None:
        self.echo("", err=err)
        for line in lines:
            self.echo(line, err=err)
        self.echo("", err=err)

    def prompt(self, message: str = "") -> str:
        prompt_message = _normalize_message(message)
        if (
            message.endswith(" ")
            and prompt_message
            and not prompt_message.endswith((" ", "\n"))
        ):
            prompt_message += " "
        return input(prompt_message)

    @contextmanager
    def status(self, message: str, *, spinner: str = "dots"):
        status_message = _normalize_message(message)
        with console.status(
            status_message,
            spinner=spinner,
            spinner_style="cyan",
        ):
            yield

    def clear(self) -> None:
        console.clear()


ui = ConsoleUI()
