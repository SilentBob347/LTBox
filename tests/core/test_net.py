from unittest.mock import MagicMock, patch

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
