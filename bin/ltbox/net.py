import time
from threading import local
from contextlib import contextmanager
from typing import Dict, Generator, Optional

import httpx

_CLIENT_LOCAL = local()


def get_client() -> httpx.Client:
    client = getattr(_CLIENT_LOCAL, "client", None)
    if client is None:
        client = httpx.Client(follow_redirects=True)
        setattr(_CLIENT_LOCAL, "client", client)
    return client


@contextmanager
def request_with_retries(
    method: str,
    url: str,
    *,
    headers: Optional[Dict[str, str]] = None,
    timeout: int = 30,
    retries: int = 3,
    backoff: float = 5,
    stream: bool = True,
    follow_redirects: bool = True,
) -> Generator[httpx.Response, None, None]:
    client = get_client()
    for attempt in range(retries + 1):
        try:
            if stream:
                with client.stream(
                    method,
                    url,
                    headers=headers,
                    timeout=timeout,
                    follow_redirects=follow_redirects,
                ) as response:
                    response.raise_for_status()
                    yield response
                return
            else:
                response = client.request(
                    method,
                    url,
                    headers=headers,
                    timeout=timeout,
                    follow_redirects=follow_redirects,
                )
                response.raise_for_status()
                yield response
                return
        except httpx.HTTPError:
            if attempt >= retries:
                raise
            time.sleep(backoff * (attempt + 1))
