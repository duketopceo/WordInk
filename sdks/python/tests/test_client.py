"""Client tests — httpx MockTransport, no network."""

from __future__ import annotations

import json

import httpx
import pytest

from wordink.cli import main
from wordink.client import (
    DEFAULT_GATEWAY_URL,
    WordInkError,
    list_models,
    resolve_config,
    transcribe,
    vocab_add,
    vocab_list,
    vocab_remove,
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


def _vocab_route(request: httpx.Request) -> httpx.Response:
    assert request.headers["Authorization"] == "Bearer wdk_admin"
    assert request.url.path == "/v1/vocabulary"
    if request.method == "GET":
        return httpx.Response(200, json={"object": "list", "data": [], "prompt_chars": 0, "keyterm_count": 0})
    return httpx.Response(200, json={"object": "list", "data": [{"term": "kurultai", "outcome": "added"}]})


def test_vocab_token_preference(monkeypatch):
    # Vocab calls prefer WORDINK_ADMIN_TOKEN; dictation prefers WORDINK_DEVICE_TOKEN.
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_device")
    _, tok = resolve_config(admin=True)
    assert tok == "wdk_admin"
    _, tok = resolve_config()
    assert tok == "wdk_device"
    # Each falls back to the other scope when its own is unset.
    monkeypatch.delenv("WORDINK_ADMIN_TOKEN")
    _, tok = resolve_config(admin=True)
    assert tok == "wdk_device"
    monkeypatch.delenv("WORDINK_DEVICE_TOKEN")
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    _, tok = resolve_config()
    assert tok == "wdk_admin"


def test_vocab_requires_admin_token_env(monkeypatch):
    monkeypatch.delenv("WORDINK_ADMIN_TOKEN", raising=False)
    monkeypatch.delenv("WORDINK_DEVICE_TOKEN", raising=False)
    with pytest.raises(WordInkError, match="no admin token"):
        resolve_config(admin=True)


def test_vocab_list(monkeypatch):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    resp = vocab_list(transport=httpx.MockTransport(_vocab_route))
    assert resp["object"] == "list"
    assert resp["data"] == []


def test_vocab_add_sends_terms_and_flags(monkeypatch):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")

    def check(request: httpx.Request) -> httpx.Response:
        assert request.method == "POST"
        assert json.loads(request.content) == {
            "terms": ["kurultai", "wisp"],
            "remove": ["old-term"],
            "dryRun": True,
        }
        return httpx.Response(200, json={"object": "list", "data": [{"term": "kurultai", "outcome": "added"}]})

    resp = vocab_add(
        ["kurultai", "wisp"], remove=["old-term"], dry_run=True, transport=httpx.MockTransport(check)
    )
    assert resp["data"][0]["outcome"] == "added"


def test_vocab_remove_sends_term_body(monkeypatch):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")

    def check(request: httpx.Request) -> httpx.Response:
        assert request.method == "DELETE"
        assert json.loads(request.content) == {"term": "Kurultai"}
        return httpx.Response(200, json={"object": "list", "data": [{"term": "Kurultai", "outcome": "removed"}]})

    resp = vocab_remove("Kurultai", transport=httpx.MockTransport(check))
    assert resp["data"][0]["outcome"] == "removed"


def test_vocab_device_token_403_surfaces_cleanly(monkeypatch):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_device")
    transport = httpx.MockTransport(
        lambda r: httpx.Response(
            403, json={"error": {"message": "admin token required", "code": "insufficient_permissions"}}
        )
    )
    with pytest.raises(WordInkError, match="403: admin token required"):
        vocab_list(transport=transport)


def test_cli_vocab_add_prints_json(monkeypatch, capsys):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    monkeypatch.setattr(
        "wordink.cli.vocab_add",
        lambda terms, **kw: {"object": "list", "data": [{"term": t, "outcome": "added"} for t in terms]},
    )
    assert main(["vocab", "add", "kurultai"]) == 0
    out = json.loads(capsys.readouterr().out)
    assert out["data"] == [{"term": "kurultai", "outcome": "added"}]


def test_cli_vocab_list_and_remove(monkeypatch, capsys):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    monkeypatch.setattr("wordink.cli.vocab_list", lambda **kw: {"object": "list", "data": []})
    monkeypatch.setattr(
        "wordink.cli.vocab_remove",
        lambda term, **kw: {"object": "list", "data": [{"term": term, "outcome": "removed"}]},
    )
    assert main(["vocab", "list"]) == 0
    assert json.loads(capsys.readouterr().out)["data"] == []
    assert main(["vocab", "remove", "kurultai"]) == 0
    assert json.loads(capsys.readouterr().out)["data"][0]["outcome"] == "removed"
