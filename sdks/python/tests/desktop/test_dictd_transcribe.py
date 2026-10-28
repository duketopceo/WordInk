"""Groq Whisper transcription: config resolution, request shape, retry, kept audio, secrecy."""

from __future__ import annotations

import os
import stat

import httpx
import pytest

from wordink.client import WordInkError
from wordink.desktop.transcribe import (
    DEFAULT_WHISPER_MODEL,
    GROQ_BASE,
    TranscribeConfig,
    TranscriptionError,
    default_failed_dir,
    resolve_transcribe_config,
    save_failed_audio,
    transcribe_audio,
)

KEY = "gsk_test_secret_key"


def _cfg(**kw) -> TranscribeConfig:
    return TranscribeConfig(GROQ_BASE, KEY, **kw)


def test_resolve_defaults_to_groq_with_turbo_model():
    cfg = resolve_transcribe_config({"GROQ_API_KEY": KEY})
    assert cfg.endpoint == GROQ_BASE and cfg.token == KEY and cfg.model == DEFAULT_WHISPER_MODEL


def test_resolve_overrides_model_and_language():
    cfg = resolve_transcribe_config({"GROQ_API_KEY": KEY}, model="whisper-large-v3", language="en")
    assert (cfg.model, cfg.language) == ("whisper-large-v3", "en")


def test_resolve_without_key_is_actionable_and_names_no_secret():
    with pytest.raises(WordInkError, match="GROQ_API_KEY is not set"):
        resolve_transcribe_config({})


def test_resolve_prefers_gateway_when_url_is_set():
    cfg = resolve_transcribe_config(
        {"WORDINK_GATEWAY_URL": "http://127.0.0.1:8941/", "WORDINK_DEVICE_TOKEN": "wdk_dev", "GROQ_API_KEY": KEY}
    )
    assert (cfg.endpoint, cfg.token, cfg.model) == ("http://127.0.0.1:8941", "wdk_dev", "wordink")


def test_resolve_gateway_without_token_fails_clearly():
    with pytest.raises(WordInkError, match="WORDINK_DEVICE_TOKEN"):
        resolve_transcribe_config({"WORDINK_GATEWAY_URL": "http://127.0.0.1:8941"})


def test_config_repr_hides_token():
    assert KEY not in repr(_cfg())


def test_request_hits_groq_with_bearer_model_language_and_vocab_prompt():
    seen: dict = {}

    def handler(request: httpx.Request) -> httpx.Response:
        seen["url"] = str(request.url)
        seen["auth"] = request.headers["Authorization"]
        seen["body"] = request.content
        return httpx.Response(200, json={"text": "  hello WordInk  "})

    text = transcribe_audio(
        b"RIFFwav", _cfg(language="en"), ["WordInk", "Hyprland"], transport=httpx.MockTransport(handler)
    )
    assert text == "hello WordInk"
    assert seen["url"] == "https://api.groq.com/openai/v1/audio/transcriptions"
    assert seen["auth"] == f"Bearer {KEY}"
    body = seen["body"]
    assert b"whisper-large-v3-turbo" in body
    assert b'name="language"' in body and b"\r\n\r\nen\r\n" in body
    assert b"WordInk, Hyprland" in body
    assert b"RIFFwav" in body


def test_no_prompt_field_when_vocabulary_empty():
    def handler(request: httpx.Request) -> httpx.Response:
        assert b'name="prompt"' not in request.content
        return httpx.Response(200, json={"text": "ok"})

    assert transcribe_audio(b"w", _cfg(), [], transport=httpx.MockTransport(handler)) == "ok"


def test_retries_once_on_429_then_succeeds():
    calls: list[int] = []
    sleeps: list[float] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(1)
        if len(calls) == 1:
            return httpx.Response(429, json={"error": {"message": "rate limited"}})
        return httpx.Response(200, json={"text": "second try"})

    out = transcribe_audio(b"w", _cfg(), transport=httpx.MockTransport(handler), sleep=sleeps.append)
    assert out == "second try" and len(calls) == 2 and len(sleeps) == 1


def test_retries_once_on_network_error():
    calls: list[int] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(1)
        if len(calls) == 1:
            raise httpx.ConnectError("boom")
        return httpx.Response(200, json={"text": "back"})

    assert transcribe_audio(b"w", _cfg(), transport=httpx.MockTransport(handler), sleep=lambda s: None) == "back"


def test_gives_up_after_one_retry_and_keeps_audio(tmp_path):
    calls: list[int] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(1)
        return httpx.Response(503, json={"error": {"message": "overloaded"}})

    with pytest.raises(TranscriptionError, match="overloaded") as info:
        transcribe_audio(
            b"RIFFkeepme", _cfg(), transport=httpx.MockTransport(handler), sleep=lambda s: None, failed_dir=tmp_path / "failed"
        )
    assert len(calls) == 2
    saved = info.value.saved_audio
    assert saved is not None and saved.read_bytes() == b"RIFFkeepme"
    assert stat.S_IMODE(saved.stat().st_mode) == 0o600
    assert stat.S_IMODE(saved.parent.stat().st_mode) == 0o700
    assert info.value.status == 503


def test_auth_failure_is_not_retried():
    calls: list[int] = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(1)
        return httpx.Response(401, json={"error": {"message": "Invalid API Key"}})

    with pytest.raises(TranscriptionError, match="Invalid API Key"):
        transcribe_audio(b"w", _cfg(), transport=httpx.MockTransport(handler), sleep=lambda s: None)
    assert len(calls) == 1


def test_error_text_never_contains_the_key():
    handler = lambda r: httpx.Response(401, json={"error": {"message": f"bad key {KEY}"}})  # noqa: E731
    with pytest.raises(TranscriptionError) as info:
        transcribe_audio(b"w", _cfg(), transport=httpx.MockTransport(handler), sleep=lambda s: None)
    assert KEY not in str(info.value)


def test_no_audio_kept_unless_failed_dir_given(tmp_path):
    handler = lambda r: httpx.Response(400, json={"error": {"message": "bad audio"}})  # noqa: E731
    with pytest.raises(TranscriptionError) as info:
        transcribe_audio(b"w", _cfg(), transport=httpx.MockTransport(handler), sleep=lambda s: None)
    assert info.value.saved_audio is None


def test_save_failed_audio_prunes_to_newest(tmp_path):
    paths = [save_failed_audio(bytes([i]), tmp_path / "f", keep=3) for i in range(5)]
    left = sorted(p.name for p in (tmp_path / "f").iterdir())
    assert left == sorted(p.name for p in paths[-3:])


def test_default_failed_dir_follows_xdg_cache(tmp_path):
    assert default_failed_dir({"XDG_CACHE_HOME": str(tmp_path)}) == tmp_path / "wordink" / "failed"
    assert os.fspath(default_failed_dir({"HOME": "/home/u"})) == "/home/u/.cache/wordink/failed"
