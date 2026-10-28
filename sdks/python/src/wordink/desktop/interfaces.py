"""The seams between the dictation pipeline and one operating system (Windows plan W1).

The state machine and the Groq pipeline never import an OS API. They talk to these protocols;
`wordink.desktop.platform` and the per-OS packages supply the implementations.
"""

from __future__ import annotations

from collections.abc import Awaitable, Callable
from typing import Protocol, runtime_checkable

#: A command line from a client ("start", "stop", "toggle", "status") -> the JSON-able reply.
CommandHandler = Callable[[str], Awaitable[dict]]


@runtime_checkable
class AudioSource(Protocol):
    """A live capture of s16 mono 16 kHz PCM. One capture at a time."""

    async def start(self) -> None: ...

    async def read_all(self) -> bytes:
        """End the capture and return every PCM byte captured since `start()`."""
        ...

    async def abort(self) -> None: ...

    @property
    def active(self) -> bool: ...


@runtime_checkable
class HotkeySource(Protocol):
    """Press/release events for the hold chord and press events for the toggle chord."""

    def start(
        self,
        on_hold_press: Callable[[], None],
        on_hold_release: Callable[[], None],
        on_toggle: Callable[[], None],
    ) -> None: ...

    def stop(self) -> None: ...


@runtime_checkable
class TextInserter(Protocol):
    """Put finished text into the focused window. Returns the method used (for the log)."""

    def insert(self, text: str) -> str: ...


@runtime_checkable
class Indicator(Protocol):
    """Show daemon state (`idle`, `recording`, `transcribing`, `error`) and surface failures."""

    def show(self, state: str, detail: str | None = None) -> None: ...


@runtime_checkable
class Transport(Protocol):
    """How a client command reaches the daemon: a Unix socket on Linux, loopback TCP on Windows."""

    async def serve(self, handler: CommandHandler) -> None: ...

    async def close(self) -> None: ...

    async def request(self, command: str, timeout: float = 3.0) -> dict:
        """Send one command to a running daemon; raises WordInkError when none answers."""
        ...

    def alive(self) -> bool:
        """True when a daemon is accepting connections (cheap probe for `doctor` and the tray)."""
        ...
