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
    gateway: str | None = None, token: str | None = None, *, admin: bool = False
) -> tuple[str, str]:
    """Flag > env > built-in default for the URL; token has no default.

    Dictation resolves --token > WORDINK_DEVICE_TOKEN > WORDINK_ADMIN_TOKEN;
    vocabulary calls resolve --token > WORDINK_ADMIN_TOKEN > WORDINK_DEVICE_TOKEN
    (a device token still works — the gateway answers a clean 403).
    """
    url = (
        gateway
        or os.environ.get("WORDINK_GATEWAY_URL")
        or DEFAULT_GATEWAY_URL
    ).rstrip("/")
    scope = "admin" if admin else "device"
    primary = f"WORDINK_{scope.upper()}_TOKEN"
    fallback = "WORDINK_DEVICE_TOKEN" if admin else "WORDINK_ADMIN_TOKEN"
    tok = token or os.environ.get(primary) or os.environ.get(fallback)
    if not tok:
        flag = " --admin" if admin else ""
        raise WordInkError(
            f"no {scope} token: pass --token or set {primary} "
            f"(mint one with `wordink-gateway tokens create{flag} <label>`)"
        )
    return url, tok


def _request_json(
    method: str,
    path: str,
    *,
    json_body: dict | None = None,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> dict:
    """One JSON round-trip against the gateway's admin surfaces (vocabulary)."""
    url, tok = resolve_config(gateway, token, admin=True)
    try:
        with httpx.Client(transport=transport, timeout=30) as http:
            resp = http.request(
                method,
                f"{url}{path}",
                headers={"Authorization": f"Bearer {tok}"},
                json=json_body,
            )
    except httpx.HTTPError as exc:
        raise WordInkError(f"request failed: {exc}") from exc
    if resp.status_code != 200:
        try:
            detail = resp.json().get("error", {}).get("message", resp.text)
        except ValueError:
            detail = resp.text
        raise WordInkError(f"{resp.status_code}: {detail}")
    return resp.json()


def vocab_list(
    *,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> dict:
    """The effective vocabulary: config-pinned terms plus runtime additions.

    Each item carries `source` ("config"|"runtime"), `pinned`, optional
    `runtimeOverride`, `effective: {prompt, keyterm}` (which merge budgets the
    term actually reaches) and runtime provenance (`addedBy`, `addedAt`).
    """
    return _request_json("GET", "/v1/vocabulary", gateway=gateway, token=token, transport=transport)


def vocab_add(
    terms: list[str],
    *,
    remove: list[str] | None = None,
    dry_run: bool = False,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> dict:
    """Add terms to the runtime vocabulary (or stage deletions via `remove`).

    Returns per-term outcomes: added / already_present / updated (respelled a
    runtime term) / pinned (already a config term) / config_shadowed (a runtime
    spelling override on a config term). `dry_run` reports without persisting.
    """
    body: dict = {"terms": list(terms)}
    if remove:
        body["remove"] = list(remove)
    if dry_run:
        body["dryRun"] = True
    return _request_json("POST", "/v1/vocabulary", json_body=body, gateway=gateway, token=token, transport=transport)


def vocab_remove(
    term: str,
    *,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> dict:
    """Remove a runtime vocabulary term (case-folded match).

    A `term_pinned` 409 means the term is config-owned in gateway.json; deleting
    a runtime spelling override reverts to the config spelling (`reverted_to`).
    """
    return _request_json("DELETE", "/v1/vocabulary", json_body={"term": term}, gateway=gateway, token=token, transport=transport)


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
