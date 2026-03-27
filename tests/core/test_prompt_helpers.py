from ltbox.prompt_helpers import prompt_index_selection, prompt_yes_no


def test_prompt_index_selection_retries_until_valid():
    responses = iter(["x", "9", "2"])
    errors: list[str] = []

    result = prompt_index_selection(
        "Select: ",
        max_index=3,
        error_message="invalid",
        input_func=lambda _prompt: next(responses),
        error_func=errors.append,
    )

    assert result == 2
    assert errors == ["invalid", "invalid"]


def test_prompt_yes_no_returns_none_when_cancel_allowed():
    result = prompt_yes_no(
        "Continue? ",
        input_func=lambda _prompt: "c",
        error_message="invalid",
        error_func=lambda _message: None,
        allow_cancel=True,
    )

    assert result is None
