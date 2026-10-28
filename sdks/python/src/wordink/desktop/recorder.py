"""Mic capture through PipeWire (or its Pulse/ALSA fallbacks) as raw 16 kHz mono PCM in memory."""

from __future__ import annotations

import asyncio
import contextlib
import io
import shutil
import wave
from collections.abc import Callable

from ..client import WordInkError

SAMPLE_RATE = 16000
SAMPLE_WIDTH = 2  # s16le, mono

#: Tried in order. Each writes raw s16le mono 16 kHz PCM to stdout.
BACKENDS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("pw-record", ("pw-record", "--raw", "--rate", "16000", "--channels", "1", "--format", "s16", "-")),
    ("parecord", ("parecord", "--raw", "--rate=16000", "--channels=1", "--format=s16le")),
    ("arecord", ("arecord", "-q", "-t", "raw", "-f", "S16_LE", "-r", "16000", "-c", "1")),
)


class RecorderError(WordInkError):
    """Capture could not start or ended without producing audio."""


def pcm_to_wav(pcm: bytes, rate: int = SAMPLE_RATE) -> bytes:
    buf = io.BytesIO()
    with wave.open(buf, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(SAMPLE_WIDTH)
        wf.setframerate(rate)
        wf.writeframes(pcm)
    return buf.getvalue()


def detect_backend(which: Callable[[str], str | None] = shutil.which) -> tuple[str, ...]:
    for name, command in BACKENDS:
        if which(name):
            return command
    names = ", ".join(n for n, _ in BACKENDS)
    raise RecorderError(f"no capture tool found: install pipewire (pw-record), or one of: {names}")


class Recorder:
    """One capture at a time. `start()` spawns the tool, `stop()` ends it and returns a WAV (or None).

    `stop()` keeps listening for `tail_seconds` first so the last word is not clipped, then
    discards anything shorter than `min_seconds` (an accidental tap on the key).
    """

    def __init__(
        self,
        command: tuple[str, ...] | None = None,
        *,
        min_seconds: float = 0.3,
        tail_seconds: float = 0.25,
        which: Callable[[str], str | None] = shutil.which,
    ) -> None:
        self._command = command
        self._which = which
        self.min_seconds = min_seconds
        self.tail_seconds = tail_seconds
        self._proc: asyncio.subprocess.Process | None = None
        self._reader: asyncio.Task[None] | None = None
        self._chunks: list[bytes] = []

    @property
    def recording(self) -> bool:
        return self._proc is not None

    @property
    def captured_seconds(self) -> float:
        return sum(len(c) for c in self._chunks) / (SAMPLE_RATE * SAMPLE_WIDTH)

    async def _read(self, stream: asyncio.StreamReader) -> None:
        while chunk := await stream.read(4096):
            self._chunks.append(chunk)

    async def start(self) -> None:
        if self._proc is not None:
            raise RecorderError("already recording")
        command = self._command or detect_backend(self._which)
        try:
            proc = await asyncio.create_subprocess_exec(
                *command,
                stdin=asyncio.subprocess.DEVNULL,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
        except OSError as exc:
            raise RecorderError(f"cannot start {command[0]}: {exc}") from exc
        assert proc.stdout is not None
        self._chunks = []
        self._proc = proc
        self._reader = asyncio.create_task(self._read(proc.stdout))

    async def _finish(self) -> int | None:
        proc, reader = self._proc, self._reader
        self._proc = self._reader = None
        if proc is None or reader is None:
            return None
        exited_early = proc.returncode is not None
        if not exited_early:
            with contextlib.suppress(ProcessLookupError):
                proc.terminate()
        try:
            await asyncio.wait_for(proc.wait(), timeout=2)
        except asyncio.TimeoutError:
            with contextlib.suppress(ProcessLookupError):
                proc.kill()
            await proc.wait()
        await reader
        return proc.returncode if exited_early else None

    async def stop(self) -> bytes | None:
        """End the capture; WAV bytes, or None when it was too short to be speech."""
        if self._proc is None:
            return None
        if self.tail_seconds > 0 and self._proc.returncode is None:
            await asyncio.sleep(self.tail_seconds)
        code = await self._finish()
        pcm = b"".join(self._chunks)
        self._chunks = []
        if not pcm and code not in (None, 0):
            raise RecorderError(f"the capture tool exited with code {code} before recording anything (is a mic available?)")
        if len(pcm) < self.min_seconds * SAMPLE_RATE * SAMPLE_WIDTH:
            return None
        return pcm_to_wav(pcm)

    async def abort(self) -> None:
        """Drop an in-progress capture (shutdown)."""
        await self._finish()
        self._chunks = []
