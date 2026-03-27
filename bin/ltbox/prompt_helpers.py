from typing import Callable, Iterable, Optional

InputFunc = Callable[[str], str]
ErrorFunc = Callable[[str], None]
PauseFunc = Callable[[], None]


def prompt_choice(
    prompt: str,
    valid_choices: Iterable[str],
    *,
    input_func: InputFunc,
    error_message: str,
    error_func: ErrorFunc,
    normalize: Optional[Callable[[str], str]] = None,
    pause_func: Optional[PauseFunc] = None,
) -> str:
    choices = set(valid_choices)
    normalize_choice = normalize or (lambda value: value)

    while True:
        choice = normalize_choice(input_func(prompt))
        if choice in choices:
            return choice

        error_func(error_message)
        if pause_func is not None:
            pause_func()


def prompt_index_selection(
    prompt: str,
    *,
    max_index: int,
    error_message: str,
    input_func: InputFunc,
    error_func: ErrorFunc,
    pause_func: Optional[PauseFunc] = None,
    min_index: int = 1,
) -> int:
    while True:
        choice = input_func(prompt).strip()
        try:
            index = int(choice)
        except ValueError:
            error_func(error_message)
            if pause_func is not None:
                pause_func()
            continue

        if min_index <= index <= max_index:
            return index

        error_func(error_message)
        if pause_func is not None:
            pause_func()


def prompt_yes_no(
    prompt: str,
    *,
    input_func: InputFunc,
    error_message: str,
    error_func: ErrorFunc,
    allow_cancel: bool = False,
) -> Optional[bool]:
    valid_choices = {"y", "n"}
    if allow_cancel:
        valid_choices.add("c")

    choice = prompt_choice(
        prompt,
        valid_choices,
        input_func=input_func,
        error_message=error_message,
        error_func=error_func,
        normalize=lambda value: value.strip().lower(),
    )
    if choice == "c":
        return None
    return choice == "y"
