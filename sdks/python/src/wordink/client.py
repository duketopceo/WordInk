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
    """The endpoint returned a non-2xx response, or the request never arrived.

    The gateway's machine-readable error code (e.g. "term_pinned",
    "vocabulary_full", "insufficient_permissions", "too_many_terms") is kept on
    `code` so callers — agents especially — can branch on it without parsing the
    message. `status` is the HTTP status when one was received.
    """

    def __init__(
        self,
        message: str,
        *,
        code: str | None = None,
        status: int | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.status = status

    def __str__(self) -> str:
        base = super().__str__()
        if self.status is not None and self.code:
            return f"{self.status} {self.code}: {base}"
        if self.status is not None:
            return f"{self.status}: {base}"
        return base


def _error_payload(resp: httpx.Response) -> tuple[str, str | None]:
    """Pull (message, code) out of an OpenAI-shaped error body, or the raw text."""
    try:
        body = resp.json()
    except ValueError:
        return resp.text, None
    err = body.get("error") if isinstance(body, dict) else None
    if not isinstance(err, dict):
        return resp.text, None
    message = err.get("message")
    code = err.get("code")
    return (
        message if isinstance(message, str) and message else resp.text,
        code if isinstance(code, str) and code else None,
    )


def _json_or_raise(resp: httpx.Response) -> dict:
    """A 2xx that is not JSON is still a protocol failure — same WordInkError shape."""
    try:
        body = resp.json()
    except ValueError:
        raise WordInkError(
            "expected a JSON response, got a non-JSON body",
            status=resp.status_code,
        ) from None
    if not isinstance(body, dict):
        raise WordInkError(
            "expected a JSON object response",
            status=resp.status_code,
        )
    return body


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
        detail, code = _error_payload(resp)
        raise WordInkError(detail, code=code, status=resp.status_code)
    return _json_or_raise(resp)


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
    via_post: bool = False,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> dict:
    """Remove a runtime vocabulary term (case-folded match).

    A `term_pinned` 409 means the term is config-owned in gateway.json; deleting
    a runtime spelling override reverts to the config spelling (`reverted_to`).
    `via_post` sends `POST {"remove": [...]}` instead of DELETE for proxies and
    clients that strip DELETE request bodies — the response shape is identical.
    """
    if via_post:
        return _request_json(
            "POST",
            "/v1/vocabulary",
            json_body={"remove": [term]},
            gateway=gateway,
            token=token,
            transport=transport,
        )
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
        detail, code = _error_payload(resp)
        raise WordInkError(detail, code=code, status=resp.status_code)
    body = _json_or_raise(resp)
    text = body.get("text")
    if not isinstance(text, str):
        raise WordInkError("the response carried no transcript text", status=resp.status_code)
    return text


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
        detail, code = _error_payload(resp)
        raise WordInkError(detail, code=code, status=resp.status_code)
    body = _json_or_raise(resp)
    return [m["id"] for m in body.get("data", [])]
