"""Loopback TCP transport with a session token (plan W7a)."""

from __future__ import annotations

import asyncio
import json
import os
import stat
import sys

import pytest

from wordink.client import WordInkError
from wordink.desktop.daemon import Daemon, PipelineResult
from wordink.desktop.config import DictateConfig
from wordink.desktop.interfaces import Transport
from wordink.desktop.win.transport import PORT_FILE, LoopbackTransport, read_endpoint


async def echo(cmd: str) -> dict:
    return {"ok": True, "echo": cmd}


async def raw(port: int, payload: bytes, timeout=2.0) -> bytes:
    r, w = await asyncio.open_connection("127.0.0.1", port)
    w.write(payload)
    await w.drain()
    try:
        return await asyncio.wait_for(r.readline(), timeout)
    finally:
        w.close()


def test_round_trip_and_the_port_file(tmp_path):
    async def go():
        t = LoopbackTransport(tmp_path / "run")
        assert isinstance(t, Transport) and not t.alive()
        await t.serve(echo)
        port, token = read_endpoint(tmp_path / "run")
        assert port == t.port and len(token) == 64
        assert t.alive()
        assert await LoopbackTransport(tmp_path / "run").request(" Toggle ") == {"ok": True, "echo": "toggle"}
        await t.close()
        assert not (tmp_path / "run" / PORT_FILE).exists() and not t.alive()

    asyncio.run(go())


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX mode bits")
def test_the_port_file_is_private(tmp_path):
    async def go():
        t = LoopbackTransport(tmp_path)
        await t.serve(echo)
        assert stat.S_IMODE(os.stat(tmp_path / PORT_FILE).st_mode) == 0o600
        await t.close()

    asyncio.run(go())


def test_a_wrong_or_missing_token_is_refused_and_never_reaches_the_handler(tmp_path):
    seen: list[str] = []

    async def spy(cmd):
        seen.append(cmd)
        return {"ok": True}

    async def go():
        t = LoopbackTransport(tmp_path)
        await t.serve(spy)
        port, token = read_endpoint(tmp_path)
        bad = json.loads(await raw(port, json.dumps({"token": "x" * 64, "command": "start"}).encode() + b"\n"))
        none = json.loads(await raw(port, json.dumps({"command": "start"}).encode() + b"\n"))
        junk = json.loads(await raw(port, b"start\n"))
        prefix = json.loads(await raw(port, json.dumps({"token": token[:-1], "command": "start"}).encode() + b"\n"))
        assert bad == {"ok": False, "error": "unauthorized"}
        assert none["error"] == junk["error"] == "bad request"
        assert prefix["error"] == "unauthorized"
        assert seen == []
        await t.close()

    asyncio.run(go())


def test_each_session_has_a_fresh_token(tmp_path):
    async def go():
        tokens = []
        for _ in range(2):
            t = LoopbackTransport(tmp_path)
            await t.serve(echo)
            tokens.append(read_endpoint(tmp_path)[1])
            await t.close()
        assert tokens[0] != tokens[1]

    asyncio.run(go())


def test_a_second_daemon_is_refused_and_a_stale_port_file_is_replaced(tmp_path):
    async def go():
        t = LoopbackTransport(tmp_path)
        await t.serve(echo)
        with pytest.raises(WordInkError, match="already running"):
            await LoopbackTransport(tmp_path).serve(echo)
        await t.close()
        (tmp_path / PORT_FILE).write_text(json.dumps({"port": 1, "token": "stale"}))  # nobody listens on port 1
        with pytest.raises(WordInkError, match="stale"):
            await LoopbackTransport(tmp_path).request("status")
        t2 = LoopbackTransport(tmp_path)
        await t2.serve(echo)  # replaces the stale file
        assert (await t2.request("status"))["ok"] is True
        await t2.close()

    asyncio.run(go())


def test_no_port_file_means_not_running(tmp_path):
    with pytest.raises(WordInkError, match="not running"):
        asyncio.run(LoopbackTransport(tmp_path).request("status"))
    assert read_endpoint(tmp_path) is None
    (tmp_path / PORT_FILE).write_text("not json")
    assert read_endpoint(tmp_path) is None


def test_it_listens_on_loopback_only(tmp_path):
    async def go():
        t = LoopbackTransport(tmp_path)
        await t.serve(echo)
        hosts = {s.getsockname()[0] for s in t._server.sockets}
        assert hosts == {"127.0.0.1"}
        await t.close()

    asyncio.run(go())


def test_the_daemon_runs_on_it(tmp_path):
    cfg = DictateConfig(runtime_dir=tmp_path / "run", vocab_path=tmp_path / "v.txt", failed_dir=tmp_path / "f")

    async def go():
        t = LoopbackTransport(cfg.runtime_dir)
        d = Daemon(cfg, lambda wav: PipelineResult("", False, "x"), lambda s: None, transport=t)
        await d.serve()
        assert (await t.request("status")) == {"ok": True, "state": "idle"}
        assert (await t.request("stop"))["note"] == "not recording"
        await d.close()

    asyncio.run(go())
