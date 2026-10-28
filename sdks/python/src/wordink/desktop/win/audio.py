"""Microphone capture on Windows through `sounddevice` (WASAPI), as 16 kHz mono s16 PCM (plan W3).

`sounddevice` is imported lazily and can be injected, so everything here runs against a fake on Linux.
"""

from __future__ import annotations

import array
import sys
import threading
from types import ModuleType

from ..recorder import SAMPLE_RATE, RecorderError


def resample_s16(pcm: bytes, src_rate: int, dst_rate: int = SAMPLE_RATE) -> bytes:
    """Linear resampler for mono s16le. Output length is `round(n * dst / src)` samples."""
    if src_rate == dst_rate or not pcm:
        return pcm
    src = array.array("h")
    src.frombytes(pcm[: len(pcm) - len(pcm) % 2])
    if sys.byteorder == "big":
        src.byteswap()
    n_out = max(1, round(len(src) * dst_rate / src_rate))
    step = src_rate / dst_rate
    out = array.array("h")
    last = len(src) - 1
    for i in range(n_out):
        pos = i * step
        j = int(pos)
        if j >= last:
            out.append(src[last])
            continue
        frac = pos - j
        out.append(int(src[j] + (src[j + 1] - src[j]) * frac))
    if sys.byteorder == "big":
        out.byteswap()
    return out.tobytes()


_NO_DEVICE = "no microphone found: plug one in or set a default input device in Settings, System, Sound"
_DENIED = "Windows blocked microphone access: turn on Settings, Privacy & security, Microphone, 'Let desktop apps access your microphone'"


def _explain(exc: Exception) -> RecorderError:
    text = str(exc).lower()
    if any(s in text for s in ("no default input", "no input device", "invalid number of channels", "device unavailable")):
        return RecorderError(_NO_DEVICE)
    if any(s in text for s in ("access", "denied", "permission", "-9985", "-9996")):
        return RecorderError(_DENIED)
    return RecorderError(f"cannot open the microphone: {exc}")


def _load_sounddevice() -> ModuleType:
    try:
        import sounddevice
    except ImportError as exc:
        raise RecorderError("microphone capture needs the 'sounddevice' package: install wordink with the 'windows' extra") from exc
    except OSError as exc:  # PortAudio library missing
        raise RecorderError(f"the audio library could not load: {exc}") from exc
    return sounddevice


class SoundDeviceSource:
    """An `AudioSource` over the default input device. One capture at a time."""

    def __init__(self, sd: ModuleType | None = None) -> None:
        self._sd = sd
        self._stream = None
        self._rate = SAMPLE_RATE
        self._chunks: list[bytes] = []
        self._lock = threading.Lock()

    @property
    def active(self) -> bool:
        s = self._stream
        return s is not None and bool(getattr(s, "active", True))

    @property
    def captured_bytes(self) -> int:
        with self._lock:
            n = sum(len(c) for c in self._chunks)
        return n if self._rate == SAMPLE_RATE else round(n * SAMPLE_RATE / self._rate)

    def _callback(self, indata, frames, time, status) -> None:  # runs on PortAudio's thread
        data = bytes(indata)
        with self._lock:
            self._chunks.append(data)

    def _open(self, sd: ModuleType, rate: int):
        stream = sd.RawInputStream(samplerate=rate, channels=1, dtype="int16", callback=self._callback)
        stream.start()
        return stream

    async def start(self) -> None:
        if self._stream is not None:
            raise RecorderError("already recording")
        sd = self._sd or _load_sounddevice()
        self._chunks = []
        try:
            try:
                self._stream, self._rate = self._open(sd, SAMPLE_RATE), SAMPLE_RATE
            except Exception as first:
                # The device may refuse 16 kHz in shared mode: capture at its own rate and resample.
                try:
                    rate = int(sd.query_devices(kind="input")["default_samplerate"])
                except Exception:
                    raise _explain(first) from first
                if rate == SAMPLE_RATE:
                    raise _explain(first) from first
                try:
                    self._stream, self._rate = self._open(sd, rate), rate
                except Exception as second:
                    raise _explain(second) from second
        except RecorderError:
            self._stream = None
            raise

    def _close(self) -> None:
        stream, self._stream = self._stream, None
        if stream is not None:
            try:
                stream.stop()
            finally:
                stream.close()

    async def read_all(self) -> bytes:
        self._close()  # stop() flushes the callback's last buffers before returning
        with self._lock:
            pcm, self._chunks = b"".join(self._chunks), []
        return resample_s16(pcm, self._rate)

    async def abort(self) -> None:
        self._close()
        with self._lock:
            self._chunks = []
