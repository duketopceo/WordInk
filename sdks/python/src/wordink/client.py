"""Gateway client: thin wrapper over the OpenAI-compatible transcriptions API.

The gateway owns provider fallback, shared vocabulary, rate limiting and auth —
this client only speaks the protocol, so it works against any compatible
endpoint, not just wordink-gateway.
"""

from __future__ import annotations

import os
from pathlib import Path

import httpx

DEFAULT_GATEWAY_URL = "http://127.0.0.1:8941"


class WordInkError(Exception):
    """The endpoint returned a non-2xx response, or the request never arrived."""


def resolve_config(
    gateway: str | None = None, token: str | None = None
) -> tuple[str, str]:
    """Flag > env > built-in default for the URL; token has no default."""
    url = (
        gateway
        or os.environ.get("WORDINK_GATEWAY_URL")
        or DEFAULT_GATEWAY_URL
    ).rstrip("/")
    tok = token or os.environ.get("WORDINK_DEVICE_TOKEN")
    if not tok:
        raise WordInkError(
            "no device token: pass --token or set WORDINK_DEVICE_TOKEN "
            "(mint one with `wordink-gateway token new`)"
        )
    return url, tok


def transcribe(
    audio: bytes | Path,
    *,
    filename: str = "audio.wav",
    content_type: str = "audio/wav",
    gateway: str | None = None,
    token: str | None = None,
    language: str | None = None,
    prompt: str | None = None,
    model: str = "wordink",
    transport: httpx.BaseTransport | None = None,
) -> str:
    """Transcribe audio bytes (or a file path) via the gateway."""
    url, tok = resolve_config(gateway, token)
    data = audio.read_bytes() if isinstance(audio, Path) else audio
    fields: dict[str, str] = {"model": model, "response_format": "json"}
    if language:
        fields["language"] = language
    if prompt:
        fields["prompt"] = prompt
    try:
        with httpx.Client(transport=transport, timeout=120) as http:
            resp = http.post(
                f"{url}/v1/audio/transcriptions",
                headers={"Authorization": f"Bearer {tok}"},
                files={"file": (filename, data, content_type)},
                data=fields,
            )
    except httpx.HTTPError as exc:
        raise WordInkError(f"request failed: {exc}") from exc
    if resp.status_code != 200:
        try:
            detail = resp.json().get("error", {}).get("message", resp.text)
        except ValueError:
            detail = resp.text
        raise WordInkError(f"{resp.status_code}: {detail}")
    return resp.json()["text"]


def list_models(
    *,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> list[str]:
    """Return the model ids the gateway currently serves."""
    url, tok = resolve_config(gateway, token)
    try:
        with httpx.Client(transport=transport, timeout=30) as http:
            resp = http.get(
                f"{url}/v1/models",
                headers={"Authorization": f"Bearer {tok}"},
            )
    except httpx.HTTPError as exc:
        raise WordInkError(f"request failed: {exc}") from exc
    if resp.status_code != 200:
        raise WordInkError(f"{resp.status_code}: {resp.text}")
    return [m["id"] for m in resp.json().get("data", [])]
