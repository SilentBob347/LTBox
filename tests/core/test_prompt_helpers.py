from ltbox.prompt_helpers import (
    prompt_index_selection,
    prompt_multi_select_indices,
    prompt_yes_no,
)


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


def test_prompt_multi_select_indices_supports_toggle_and_finish():
    responses = iter(["1", "3", "f"])
    renders: list[set[int]] = []

    result = prompt_multi_select_indices(
        "Select: ",
        item_count=3,
        render_func=lambda selected: renders.append(set(selected)),
        input_func=lambda _prompt: next(responses),
        error_message="invalid",
        error_func=lambda _message: None,
    )

    assert result == [0, 2]
    assert renders == [set(), {0}, {0, 2}]


def test_prompt_multi_select_indices_supports_all_and_cancel():
    responses = iter(["a", "c"])

    result = prompt_multi_select_indices(
        "Select: ",
        item_count=2,
        render_func=lambda _selected: None,
        input_func=lambda _prompt: next(responses),
        error_message="invalid",
        error_func=lambda _message: None,
        select_all_choice="a",
    )

    assert result is None
