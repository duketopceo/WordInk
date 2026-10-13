"""MCP tool tests — the decorated functions are exercised directly (no stdio)."""

from __future__ import annotations

import asyncio
import json

import pytest

from wordink.client import WordInkError
from wordink import mcp_server


def run(coro):
    return asyncio.run(coro)


def test_vocab_list_returns_json(monkeypatch):
    monkeypatch.setattr(
        mcp_server,
        "vocab_list",
        lambda **kw: {"object": "list", "data": [{"term": "kurultai", "source": "runtime"}]},
    )
    out = run(mcp_server.vocab_list_tool())
    assert json.loads(out)["data"][0]["term"] == "kurultai"


def test_vocab_add_passes_terms_and_dry_run(monkeypatch):
    seen = {}

    def fake_add(terms, **kw):
        seen.update(kw)
        return {"object": "list", "data": [{"term": t, "outcome": "added"} for t in terms]}

    monkeypatch.setattr(mcp_server, "vocab_add", fake_add)
    out = run(mcp_server.vocab_add_tool(["kurultai"], dry_run=True))
    assert json.loads(out)["data"][0]["outcome"] == "added"
    assert seen == {"dry_run": True}


def test_vocab_remove_passes_via_post(monkeypatch):
    seen = {}

    def fake_remove(term, **kw):
        seen["term"] = term
        seen.update(kw)
        return {"object": "list", "data": [{"term": term, "outcome": "removed"}]}

    monkeypatch.setattr(mcp_server, "vocab_remove", fake_remove)
    out = run(mcp_server.vocab_remove_tool("kurultai", via_post=True))
    assert json.loads(out)["data"][0]["outcome"] == "removed"
    assert seen == {"term": "kurultai", "via_post": True}


def test_tools_surface_wordink_errors_as_text(monkeypatch):
    def boom(**kw):
        raise WordInkError("Kurultai is config-pinned", code="term_pinned", status=409)

    monkeypatch.setattr(mcp_server, "vocab_list", boom)
    monkeypatch.setattr(mcp_server, "vocab_add", lambda terms, **kw: boom())
    monkeypatch.setattr(mcp_server, "vocab_remove", lambda term, **kw: boom())

    # The gateway's error code must reach the agent, not just a human message.
    assert run(mcp_server.vocab_list_tool()) == "wordink: 409 term_pinned: Kurultai is config-pinned"
    assert "term_pinned" in run(mcp_server.vocab_add_tool(["x"]))
    assert "term_pinned" in run(mcp_server.vocab_remove_tool("x"))


def test_main_env_gate(monkeypatch):
    for var in ("WORDINK_DEVICE_TOKEN", "WORDINK_ADMIN_TOKEN"):
        monkeypatch.delenv(var, raising=False)
    with pytest.raises(SystemExit, match="WORDINK_DEVICE_TOKEN"):
        mcp_server.main()

    # An admin-only env still opens the server — vocab tools need exactly that.
    ran = {}
    monkeypatch.setattr(mcp_server.server, "run", lambda *a, **kw: ran.setdefault("ran", True))
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    mcp_server.main()
    assert ran["ran"] is True
