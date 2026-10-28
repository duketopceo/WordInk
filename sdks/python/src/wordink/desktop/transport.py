"""How a client command reaches the daemon. `UnixTransport` is the Linux one (Windows plan W1c)."""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import socket
from pathlib import Path

from ..client import WordInkError
from .interfaces import CommandHandler


class UnixTransport:
    """One line in ("start", "stop", "toggle", "status"), one JSON line out, over a 0600 Unix socket."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self._server: asyncio.Server | None = None
        self._handler: CommandHandler | None = None

    # -- server ---------------------------------------------------------------------------------
    def _claim(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(self.path.parent, 0o700)
        if self.path.exists():
            if self.alive():
                raise WordInkError(f"wordink-dictd is already running (socket {self.path})")
            self.path.unlink(missing_ok=True)  # stale: nobody is listening

    async def _client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        assert self._handler is not None
        try:
            line = await asyncio.wait_for(reader.readline(), timeout=5)
            reply = await self._handler(line.decode("utf-8", "replace").strip().lower())
            writer.write((json.dumps(reply) + "\n").encode())
            await writer.drain()
        except (asyncio.TimeoutError, ConnectionError):
            pass
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError):
                await writer.wait_closed()

    async def serve(self, handler: CommandHandler) -> None:
        self._claim()
        self._handler = handler
        self._server = await asyncio.start_unix_server(self._client, path=str(self.path))
        os.chmod(self.path, 0o600)

    async def close(self) -> None:
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()
            self._server = None
        self.path.unlink(missing_ok=True)

    # -- client ---------------------------------------------------------------------------------
    def alive(self) -> bool:
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        probe.settimeout(1)
        try:
            probe.connect(str(self.path))
        except OSError:
            return False
        finally:
            probe.close()
        return True

    async def request(self, command: str, timeout: float = 3.0) -> dict:
        try:
            reader, writer = await asyncio.wait_for(asyncio.open_unix_connection(str(self.path)), timeout)
        except (OSError, asyncio.TimeoutError) as exc:
            raise WordInkError(
                f"wordink-dictd is not running (socket {self.path}); start it with `wordink-dictd run`"
            ) from exc
        try:
            writer.write(f"{command}\n".encode())
            await writer.drain()
            line = await asyncio.wait_for(reader.readline(), timeout)
            return json.loads(line)
        except (asyncio.TimeoutError, ValueError, ConnectionError) as exc:
            raise WordInkError("wordink-dictd did not answer") from exc
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError):
                await writer.wait_closed()
