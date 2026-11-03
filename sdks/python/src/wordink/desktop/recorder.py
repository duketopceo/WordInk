"""Mic capture through PipeWire (or its Pulse/ALSA fallbacks) as raw 16 kHz mono PCM in memory."""

from __future__ import annotations

import asyncio
import contextlib
import io
import shutil
import wave
from collections.abc import Callable

from ..client import WordInkError
from .interfaces import AudioSource

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


class SubprocessSource:
    """An `AudioSource` that reads raw s16 mono 16 kHz PCM from a capture tool's stdout (Linux)."""

    def __init__(
        self,
        command: tuple[str, ...] | None = None,
        *,
        device: str | None = None,
        which: Callable[[str], str | None] = shutil.which,
    ) -> None:
        self._command = command
        self._device = device
        self._which = which
        self._proc: asyncio.subprocess.Process | None = None
        self._reader: asyncio.Task[None] | None = None
        self._chunks: list[bytes] = []

    @property
    def started(self) -> bool:
        return self._proc is not None

    @property
    def active(self) -> bool:
        """True while the tool is still running (it may have exited on its own)."""
        return self._proc is not None and self._proc.returncode is None

    @property
    def captured_bytes(self) -> int:
        return sum(len(c) for c in self._chunks)

    async def _read(self, stream: asyncio.StreamReader) -> None:
        while chunk := await stream.read(4096):
            self._chunks.append(chunk)

    async def start(self) -> None:
        if self._proc is not None:
            raise RecorderError("already recording")
        command = self._command or detect_backend(self._which)
        if self._device and not self._command:
            # An explicit [record] command already carries its own device flags.
            if command[0].startswith(("pw-record", "parecord")):
                command = (*command, f"--device={self._device}")
            elif command[0].startswith("arecord"):
                command = (*command, "-D", self._device)
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

    async def read_all(self) -> bytes:
        """End the capture and return its PCM; raises when the tool died before recording anything."""
        code = await self._finish()
        pcm = b"".join(self._chunks)
        self._chunks = []
        if not pcm and code not in (None, 0):
            raise RecorderError(f"the capture tool exited with code {code} before recording anything (is a mic available?)")
        return pcm

    async def abort(self) -> None:
        await self._finish()
        self._chunks = []


class Recorder:
    """One capture at a time. `start()` opens the source, `stop()` ends it and returns a WAV (or None).

    `stop()` keeps listening for `tail_seconds` first so the last word is not clipped, then
    discards anything shorter than `min_seconds` (an accidental tap on the key).
    The default source runs a PipeWire/Pulse/ALSA tool; pass `source=` to capture another way.
    """

    def __init__(
        self,
        command: tuple[str, ...] | None = None,
        *,
        device: str | None = None,
        min_seconds: float = 0.3,
        tail_seconds: float = 0.25,
        which: Callable[[str], str | None] = shutil.which,
        source: AudioSource | None = None,
    ) -> None:
        self._source: AudioSource = source if source is not None else SubprocessSource(command, device=device, which=which)
        self.min_seconds = min_seconds
        self.tail_seconds = tail_seconds
        self._recording = False

    @property
    def recording(self) -> bool:
        return self._recording

    @property
    def captured_seconds(self) -> float:
        return self._source.captured_bytes / (SAMPLE_RATE * SAMPLE_WIDTH)

    async def start(self) -> None:
        if self._recording:
            raise RecorderError("already recording")
        await self._source.start()
        self._recording = True

    async def stop(self) -> bytes | None:
        """End the capture; WAV bytes, or None when it was too short to be speech."""
        if not self._recording:
            return None
        if self.tail_seconds > 0 and self._source.active:
            await asyncio.sleep(self.tail_seconds)
        self._recording = False
        pcm = await self._source.read_all()
        if len(pcm) < self.min_seconds * SAMPLE_RATE * SAMPLE_WIDTH:
            return None
        return pcm_to_wav(pcm)

    async def abort(self) -> None:
        """Drop an in-progress capture (shutdown)."""
        self._recording = False
        await self._source.abort()
