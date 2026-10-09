"""Client tests — httpx MockTransport, no network."""

from __future__ import annotations

import httpx
import pytest

from wordink.cli import main
from wordink.client import (
    DEFAULT_GATEWAY_URL,
    WordInkError,
    list_models,
    resolve_config,
    transcribe,
)


def _ok(request: httpx.Request) -> httpx.Response:
    assert request.headers["Authorization"] == "Bearer wdk_test"
    assert request.url.path == "/v1/audio/transcriptions"
    body = request.content
    assert b'name="file"' in body and b'name="model"' in body
    return httpx.Response(200, json={"text": "hello world"})


def test_transcribe_happy(monkeypatch):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_test")
    transport = httpx.MockTransport(_ok)
    assert transcribe(b"RIFF...", transport=transport) == "hello world"


def test_transcribe_auth_error_surfaces(monkeypatch):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_bad")
    transport = httpx.MockTransport(
        lambda r: httpx.Response(
            401, json={"error": {"message": "invalid device token"}}
        )
    )
    with pytest.raises(WordInkError, match="invalid device token"):
        transcribe(b"x", transport=transport)


def test_transcribe_optional_fields(monkeypatch):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_test")

    def check(request: httpx.Request) -> httpx.Response:
        assert b'name="language"' in request.content
        assert b'name="prompt"' in request.content
        return httpx.Response(200, json={"text": "t"})

    transcribe(
        b"x",
        transport=httpx.MockTransport(check),
        language="en",
        prompt="Kurultai",
    )


def test_resolve_config_precedence(monkeypatch):
    monkeypatch.setenv("WORDINK_GATEWAY_URL", "http://env:1/g")
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_env")
    url, tok = resolve_config()
    assert (url, tok) == ("http://env:1/g", "wdk_env")
    url, tok = resolve_config(gateway="http://flag:2/g/", token="wdk_flag")
    assert (url, tok) == ("http://flag:2/g", "wdk_flag")
    monkeypatch.delenv("WORDINK_GATEWAY_URL")
    url, _ = resolve_config()
    assert url == DEFAULT_GATEWAY_URL


def test_resolve_config_requires_token(monkeypatch):
    monkeypatch.delenv("WORDINK_DEVICE_TOKEN", raising=False)
    with pytest.raises(WordInkError, match="no device token"):
        resolve_config()


def test_list_models(monkeypatch):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_test")
    transport = httpx.MockTransport(
        lambda r: httpx.Response(
            200,
            json={
                "object": "list",
                "data": [
                    {"id": "whisper-large-v3-turbo", "object": "model"},
                    {"id": "nova-3", "object": "model"},
                ],
            },
        )
    )
    assert list_models(transport=transport) == [
        "whisper-large-v3-turbo",
        "nova-3",
    ]


def test_cli_transcribe(monkeypatch, tmp_path, capsys):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_test")
    audio = tmp_path / "a.m4a"
    audio.write_bytes(b"fake")
    monkeypatch.setattr(
        "wordink.cli.transcribe", lambda *a, **kw: "from the cli"
    )
    assert main(["transcribe", str(audio)]) == 0
    assert capsys.readouterr().out.strip() == "from the cli"


def test_cli_error_exit(monkeypatch, tmp_path, capsys):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_test")
    audio = tmp_path / "a.wav"
    audio.write_bytes(b"fake")

    def boom(*a, **kw):
        raise WordInkError("401: bad token")

    monkeypatch.setattr("wordink.cli.transcribe", boom)
    assert main(["transcribe", str(audio)]) == 1
    assert "bad token" in capsys.readouterr().err
