import logging
import re
from contextlib import contextmanager
from typing import Optional

from rich.console import Console

LOGGER_NAME = "ltbox"
_logger = logging.getLogger(LOGGER_NAME)
_logger.setLevel(logging.INFO)

console = Console(highlight=False)


class RichConsoleHandler(logging.Handler):
    """Logging handler that routes messages through rich.Console."""

    STYLE_MAP = {
        "[+]": "green",
        "[*]": "cyan",
        "[-]": "yellow",
        "[X]": "red",
        "[Log]": "dim",
    }
    _BRACKET_TOKEN_PATTERN = re.compile(r"^\[([^\]]+)\]")
    _NOTICE_KEYWORDS = (
        "notice",
        "action required",
        "skip adb",
        "알림",
        "조치 필요",
        "주의",
        "需要操作",
        "注意",
        "важно",
        "требуется",
        "предупреждение",
    )
    _STEP_KEYWORDS = ("step", "단계", "步骤", "шаг")

    def emit(self, record: logging.LogRecord) -> None:
        try:
            msg = self.format(record)
            style = self._detect_style(msg, record)
            if style:
                console.print(msg, style=style, highlight=False, markup=False)
            else:
                console.print(msg, highlight=False, markup=False)
        except Exception:
            self.handleError(record)

    @staticmethod
    def _detect_style(msg: str, record: logging.LogRecord) -> Optional[str]:
        stripped = msg.lstrip()
        for prefix, style in RichConsoleHandler.STYLE_MAP.items():
            if stripped.startswith(prefix):
                return style
        if stripped.startswith("[!]"):
            return "red" if record.levelno >= logging.ERROR else "yellow"
        token_match = RichConsoleHandler._BRACKET_TOKEN_PATTERN.match(stripped)
        if token_match:
            token = token_match.group(1).casefold()
            if any(keyword in token for keyword in RichConsoleHandler._NOTICE_KEYWORDS):
                return "yellow"
            if any(
                keyword in token for keyword in RichConsoleHandler._STEP_KEYWORDS
            ) or any(character.isdigit() for character in token):
                return "cyan"
        if record.levelno >= logging.ERROR:
            return "red"
        if record.levelno >= logging.WARNING:
            return "yellow"
        return None


if not _logger.handlers:
    _rich_handler = RichConsoleHandler()
    _rich_handler.setFormatter(logging.Formatter("%(message)s"))
    _logger.addHandler(_rich_handler)


def get_logger() -> logging.Logger:
    return _logger


@contextmanager
def logging_context(log_filename: Optional[str] = None):
    handlers_to_remove = []

    has_file_handler = any(isinstance(h, logging.FileHandler) for h in _logger.handlers)

    try:
        if log_filename and not has_file_handler:
            file_handler = logging.FileHandler(log_filename, encoding="utf-8")
            file_handler.setFormatter(
                logging.Formatter("%(asctime)s - %(message)s", datefmt="%H:%M:%S")
            )
            _logger.addHandler(file_handler)
            handlers_to_remove.append(file_handler)

        yield _logger

    finally:
        for handler in handlers_to_remove:
            handler.close()
            _logger.removeHandler(handler)
