from typing import List

from .logger import console, get_logger

logger = get_logger()


def _leading_space_count(value: str) -> int:
    return len(value) - len(value.lstrip(" "))


def _is_separator_line(value: str) -> bool:
    stripped = value.strip()
    return bool(stripped) and len(set(stripped)) == 1 and stripped[0] in "=-!_"


def _is_header_only_line(value: str) -> bool:
    stripped = value.strip()
    return stripped.startswith("[") and stripped.endswith("]")


def _normalize_paragraph(lines: List[str]) -> List[str]:
    normalized_lines: List[str] = []
    buffer = ""

    for raw_line in lines:
        stripped = raw_line.strip()
        if not stripped:
            continue

        if _is_separator_line(stripped) or _is_header_only_line(stripped):
            if buffer:
                normalized_lines.append(buffer)
                buffer = ""
            normalized_lines.append(stripped)
            continue

        if not buffer:
            buffer = stripped
            continue

        if _leading_space_count(raw_line) > 0:
            buffer = f"{buffer} {stripped}"
            continue

        normalized_lines.append(buffer)
        buffer = stripped

    if buffer:
        normalized_lines.append(buffer)

    return normalized_lines


def _normalize_message(message: str) -> str:
    if not message:
        return message

    normalized = message.replace("\r\n", "\n")
    leading_blank = normalized.startswith("\n")
    trailing_blank = normalized.endswith("\n")
    core = normalized.strip("\n")

    if not core:
        return ""

    lines = core.split("\n")
    paragraph: List[str] = []
    output_lines: List[str] = []

    for line in lines:
        if line.strip():
            paragraph.append(line.rstrip())
            continue

        if paragraph:
            output_lines.extend(_normalize_paragraph(paragraph))
            paragraph.clear()
        if output_lines and output_lines[-1] != "":
            output_lines.append("")

    if paragraph:
        output_lines.extend(_normalize_paragraph(paragraph))

    cleaned = "\n".join(output_lines).rstrip()
    if leading_blank:
        cleaned = "\n" + cleaned
    if trailing_blank:
        cleaned += "\n"
    return cleaned


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

    def clear(self) -> None:
        console.clear()


ui = ConsoleUI()
