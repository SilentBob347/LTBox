import subprocess
from typing import Any

from .. import utils


def echo(message: str) -> None:
    utils.ui.echo(message)


def error(message: str) -> None:
    utils.ui.error(message)


def info(message: str) -> None:
    utils.ui.info(message)


def prompt(message: str) -> str:
    return utils.ui.prompt(message)


def clear() -> None:
    utils.ui.clear()


def get_term_width() -> int:
    return utils.ui.get_term_width()


def read_input(message: str) -> str:
    return input(message)


def run(*args: Any, **kwargs: Any) -> subprocess.CompletedProcess:
    return subprocess.run(*args, **kwargs)
