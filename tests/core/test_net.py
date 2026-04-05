from unittest.mock import MagicMock, patch

import httpx
import pytest

from ltbox import net


def test_request_with_retries_uses_thread_local_client():
    client = MagicMock()
    response = MagicMock()
    response.raise_for_status.return_value = None

    stream_cm = MagicMock()
    stream_cm.__enter__.return_value = response
    stream_cm.__exit__.return_value = False
    client.stream.return_value = stream_cm

    with patch("ltbox.net.get_client", return_value=client):
        with net.request_with_retries("GET", "https://example.com") as result:
            assert result is response

    client.stream.assert_called_once_with(
        "GET",
        "https://example.com",
        headers=None,
        timeout=30,
        follow_redirects=True,
    )


def test_get_client_reuses_client_within_thread():
    with patch("ltbox.net.httpx.Client") as client_factory:
        client_factory.return_value = MagicMock()
        net._CLIENT_LOCAL = type(net._CLIENT_LOCAL)()

        first = net.get_client()
        second = net.get_client()

    assert first is second
    client_factory.assert_called_once_with(follow_redirects=True)


def test_request_with_retries_succeeds_after_transient_failures():
    """Verify retry logic: 2 failures then success on 3rd attempt."""
    client = MagicMock()
    response = MagicMock()
    response.raise_for_status.return_value = None

    call_count = 0

    def stream_side_effect(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count <= 2:
            raise httpx.ConnectError("connection refused")
        cm = MagicMock()
        cm.__enter__ = MagicMock(return_value=response)
        cm.__exit__ = MagicMock(return_value=False)
        return cm

    client.stream.side_effect = stream_side_effect

    with (
        patch("ltbox.net.get_client", return_value=client),
        patch("ltbox.net.time.sleep") as mock_sleep,
    ):
        with net.request_with_retries(
            "GET", "https://example.com", retries=3, backoff=1
        ) as result:
            assert result is response

    assert call_count == 3
    assert mock_sleep.call_count == 2


def test_request_with_retries_exhausts_retries_then_raises():
    """When all retries fail, the last exception should propagate."""
    client = MagicMock()
    client.stream.side_effect = httpx.ConnectError("connection refused")

    with (
        patch("ltbox.net.get_client", return_value=client),
        patch("ltbox.net.time.sleep"),
    ):
        with pytest.raises(httpx.ConnectError):
            with net.request_with_retries(
                "GET", "https://example.com", retries=2, backoff=0
            ) as _:
                pass


def test_request_with_retries_non_stream_mode():
    """Test the non-stream (direct request) path."""
    client = MagicMock()
    response = MagicMock()
    response.raise_for_status.return_value = None
    client.request.return_value = response

    with patch("ltbox.net.get_client", return_value=client):
        with net.request_with_retries(
            "GET", "https://example.com", stream=False
        ) as result:
            assert result is response

    client.request.assert_called_once()
