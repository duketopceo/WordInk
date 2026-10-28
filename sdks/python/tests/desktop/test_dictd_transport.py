"""The Transport seam: the daemon runs on any transport, and the Unix one keeps its contract."""

from __future__ import annotations

import asyncio
import os
import stat

import pytest

from wordink.client import WordInkError
from wordink.desktop.config import DictateConfig
from wordink.desktop.daemon import Daemon, PipelineResult
from wordink.desktop.interfaces import CommandHandler, Transport
from wordink.desktop.transport import UnixTransport

from dictd_helpers import short_tmp


class FakeTransport:
    def __init__(self) -> None:
        self.handler: CommandHandler | None = None
        self.closed = False

    async def serve(self, handler: CommandHandler) -> None:
        self.handler = handler

    async def close(self) -> None:
        self.closed = True

    async def request(self, command: str, timeout: float = 3.0) -> dict:
        assert self.handler is not None
        return await self.handler(command)

    def alive(self) -> bool:
        return self.handler is not None and not self.closed


def test_daemon_serves_commands_through_any_transport(tmp_path):
    cfg = DictateConfig(runtime_dir=tmp_path / "run", vocab_path=tmp_path / "v.txt", failed_dir=tmp_path / "f")
    fake = FakeTransport()
    assert isinstance(fake, Transport)

    async def go() -> None:
        daemon = Daemon(cfg, lambda wav: PipelineResult("", False, "x"), lambda t: None, transport=fake)
        await daemon.serve()
        assert daemon.state == "idle" and fake.alive()
        reply = await fake.request("status")
        assert reply == {"ok": True, "state": "idle"}
        assert (await fake.request("stop"))["note"] == "not recording"
        assert (await fake.request("bogus"))["ok"] is False
        await daemon.close()
        assert fake.closed and not fake.alive()

    asyncio.run(go())
    assert not cfg.socket_path.exists()  # a custom transport never creates the Unix socket


def test_unix_transport_round_trip_and_permissions():
    root = short_tmp()
    path = root / "run" / "d.sock"

    async def handler(cmd: str) -> dict:
        return {"ok": True, "echo": cmd}

    async def go() -> None:
        t = UnixTransport(path)
        assert not t.alive()
        await t.serve(handler)
        assert t.alive()
        assert stat.S_IMODE(os.stat(path).st_mode) == 0o600
        assert stat.S_IMODE(os.stat(path.parent).st_mode) == 0o700
        assert await UnixTransport(path).request("Toggle") == {"ok": True, "echo": "toggle"}
        with pytest.raises(WordInkError, match="already running"):
            await UnixTransport(path).serve(handler)
        await t.close()
        assert not path.exists() and not t.alive()

    asyncio.run(go())


def test_unix_transport_replaces_a_stale_socket_and_reports_no_daemon():
    root = short_tmp()
    path = root / "d.sock"

    async def handler(cmd: str) -> dict:
        return {"ok": True}

    async def go() -> None:
        with pytest.raises(WordInkError, match="not running"):
            await UnixTransport(path).request("status")
        import socket

        stale = socket.socket(socket.AF_UNIX)
        stale.bind(str(path))
        stale.close()  # file stays, nobody listens
        assert path.exists()
        t = UnixTransport(path)
        await t.serve(handler)
        assert (await t.request("status"))["ok"] is True
        await t.close()

    asyncio.run(go())
