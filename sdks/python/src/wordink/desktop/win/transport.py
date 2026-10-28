"""Loopback TCP transport for the Windows daemon (plan W7a).

asyncio has no usable Unix-socket server on Windows, so the daemon listens on 127.0.0.1 with an
ephemeral port. The port and a fresh per-session token go in a file in the user's own profile
(`run_dir/dictd.port`); a client must read that file and present the token, so another local user
or a web page probing localhost cannot drive the microphone. Loopback only, token compared in
constant time. Pure asyncio and files: it runs and is tested on Linux too.
"""

from __future__ import annotations

import asyncio
import contextlib
import hmac
import ipaddress
import json
import os
import secrets
import socket
from pathlib import Path

from ...client import WordInkError
from ..interfaces import CommandHandler

PORT_FILE = "dictd.port"
MAX_LINE = 4096


def _write_private(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)  # the mode bits are a no-op on Windows; the profile ACL applies
    with os.fdopen(fd, "w", encoding="utf-8") as fh:
        fh.write(text)
    os.replace(tmp, path)


def read_endpoint(run_dir: Path) -> tuple[int, str] | None:
    try:
        data = json.loads((run_dir / PORT_FILE).read_text(encoding="utf-8"))
        return int(data["port"]), str(data["token"])
    except (OSError, ValueError, KeyError, TypeError):
        return None


def _is_loopback(host: str) -> bool:
    try:
        return ipaddress.ip_address(host.split("%", 1)[0]).is_loopback
    except ValueError:
        return False


class LoopbackTransport:
    def __init__(self, run_dir: Path) -> None:
        self.run_dir = run_dir
        self._server: asyncio.Server | None = None
        self._handler: CommandHandler | None = None
        self._token = ""

    @property
    def port_file(self) -> Path:
        return self.run_dir / PORT_FILE

    @property
    def port(self) -> int | None:
        if self._server is None or not self._server.sockets:
            return None
        return self._server.sockets[0].getsockname()[1]

    # -- server ---------------------------------------------------------------------------------
    async def _client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        assert self._handler is not None
        try:
            peer = writer.get_extra_info("peername")
            if not peer or not _is_loopback(str(peer[0])):
                return
            raw = await asyncio.wait_for(reader.readline(), timeout=5)
            if len(raw) > MAX_LINE:
                return
            try:
                msg = json.loads(raw)
                token, command = str(msg["token"]), str(msg["command"])
            except (ValueError, KeyError, TypeError):
                reply: dict = {"ok": False, "error": "bad request"}
            else:
                if hmac.compare_digest(token.encode(), self._token.encode()):
                    reply = await self._handler(command.strip().lower())
                else:
                    reply = {"ok": False, "error": "unauthorized"}
            writer.write((json.dumps(reply) + "\n").encode())
            await writer.drain()
        except (asyncio.TimeoutError, ConnectionError):
            pass
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError, OSError):
                await writer.wait_closed()

    async def serve(self, handler: CommandHandler) -> None:
        if self.alive():
            raise WordInkError(f"wordink-dictd is already running (port file {self.port_file})")
        self._handler = handler
        self._token = secrets.token_hex(32)
        self._server = await asyncio.start_server(self._client, host="127.0.0.1", port=0)
        _write_private(self.port_file, json.dumps({"port": self.port, "token": self._token}))

    async def close(self) -> None:
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()
            self._server = None
        self.port_file.unlink(missing_ok=True)

    # -- client ---------------------------------------------------------------------------------
    def alive(self) -> bool:
        endpoint = read_endpoint(self.run_dir)
        if endpoint is None:
            return False
        probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        probe.settimeout(1)
        try:
            probe.connect(("127.0.0.1", endpoint[0]))
        except OSError:
            return False
        finally:
            probe.close()
        return True

    async def request(self, command: str, timeout: float = 3.0) -> dict:
        endpoint = read_endpoint(self.run_dir)
        if endpoint is None:
            raise WordInkError("wordink-dictd is not running (no port file); start it from the tray app or `wordink-dictd run`")
        port, token = endpoint
        try:
            reader, writer = await asyncio.wait_for(asyncio.open_connection("127.0.0.1", port), timeout)
        except (OSError, asyncio.TimeoutError) as exc:
            raise WordInkError(f"wordink-dictd is not running (nothing on port {port}; the port file is stale)") from exc
        try:
            writer.write((json.dumps({"token": token, "command": command}) + "\n").encode())
            await writer.drain()
            line = await asyncio.wait_for(reader.readline(), timeout)
            return json.loads(line)
        except (asyncio.TimeoutError, ValueError, ConnectionError) as exc:
            raise WordInkError("wordink-dictd did not answer") from exc
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError, OSError):
                await writer.wait_closed()
