"""Windows microphone source against a fake `sounddevice` (plan W3)."""

from __future__ import annotations

import array
import asyncio
import sys
import types

import pytest

from wordink.desktop.interfaces import AudioSource
from wordink.desktop.recorder import Recorder, RecorderError
from wordink.desktop.win.audio import SoundDeviceSource, resample_s16


def tone(n: int, rate_hint: int = 0) -> bytes:
    a = array.array("h", [(i * 37) % 2000 - 1000 for i in range(n)])
    return a.tobytes()


class FakeStream:
    def __init__(self, sd, samplerate, callback, **_kw) -> None:
        self.sd, self.rate, self.cb = sd, samplerate, callback
        self.active = False
        self.closed = False
        if samplerate in sd.refuse_rates:
            raise sd.PortAudioError("Invalid sample rate [PaErrorCode -9997]")
        if sd.fail_open:
            raise sd.PortAudioError(sd.fail_open)

    def start(self):
        self.active = True

    def stop(self):
        self.active = False

    def close(self):
        self.closed = True

    def feed(self, data: bytes):
        self.cb(memoryview(data), len(data) // 2, None, None)


def fake_sd(refuse_rates=(), fail_open=None, default_rate=48000):
    sd = types.SimpleNamespace(refuse_rates=set(refuse_rates), fail_open=fail_open, streams=[])

    class PortAudioError(Exception):
        pass

    sd.PortAudioError = PortAudioError

    def RawInputStream(**kw):
        s = FakeStream(sd, **kw)
        sd.streams.append(s)
        return s

    sd.RawInputStream = RawInputStream
    sd.query_devices = lambda kind=None: {"default_samplerate": float(default_rate)}
    return sd


def test_it_is_an_audio_source_and_chunks_arrive_in_order():
    sd = fake_sd()
    src = SoundDeviceSource(sd)
    assert isinstance(src, AudioSource)

    async def go():
        await src.start()
        assert src.active
        s = sd.streams[-1]
        a, b, c = tone(100), tone(50), tone(10)
        for chunk in (a, b, c):
            s.feed(chunk)
        assert src.captured_bytes == 320
        pcm = await src.read_all()
        assert pcm == a + b + c and s.closed and not src.active
        return pcm

    asyncio.run(go())
    assert sd.streams[-1].rate == 16000


def test_stop_flushes_and_a_second_capture_starts_clean():
    sd = fake_sd()
    src = SoundDeviceSource(sd)

    async def go():
        await src.start()
        sd.streams[-1].feed(tone(10))
        await src.read_all()
        await src.start()
        assert src.captured_bytes == 0
        sd.streams[-1].feed(tone(4))
        assert len(await src.read_all()) == 8
        with pytest.raises(RecorderError):
            await src.start() or await src.start()

    asyncio.run(go())


def test_a_device_that_refuses_16k_is_captured_at_its_rate_and_resampled():
    sd = fake_sd(refuse_rates={16000}, default_rate=48000)
    src = SoundDeviceSource(sd)

    async def go():
        await src.start()
        assert sd.streams[-1].rate == 48000
        sd.streams[-1].feed(tone(4800))  # 0.1 s at 48 kHz
        assert src.captured_bytes == 3200
        return await src.read_all()

    pcm = asyncio.run(go())
    assert len(pcm) // 2 == 1600  # 0.1 s at 16 kHz


@pytest.mark.parametrize(
    ("error", "needle"),
    [
        ("Error querying device -1: No Default Input Device Available", "no microphone found"),
        ("Error opening RawInputStream: Access denied [PaErrorCode -9985]", "Settings, Privacy"),
        ("Something odd", "cannot open the microphone"),
    ],
)
def test_open_errors_are_actionable(error, needle):
    src = SoundDeviceSource(fake_sd(fail_open=error, default_rate=16000))
    with pytest.raises(RecorderError, match=needle):
        asyncio.run(src.start())
    assert not src.active


def test_a_failed_start_leaves_the_source_reusable():
    sd = fake_sd(fail_open="Access denied")
    src = SoundDeviceSource(sd)
    with pytest.raises(RecorderError):
        asyncio.run(src.start())
    sd.fail_open = None
    asyncio.run(src.start())
    assert src.active


def test_abort_drops_the_capture():
    sd = fake_sd()
    src = SoundDeviceSource(sd)

    async def go():
        await src.start()
        sd.streams[-1].feed(tone(10))
        await src.abort()
        assert not src.active and src.captured_bytes == 0 and sd.streams[-1].closed
        await src.abort()  # idempotent

    asyncio.run(go())


def test_the_recorder_runs_on_it_with_min_and_tail():
    sd = fake_sd()

    async def go():
        rec = Recorder(source=SoundDeviceSource(sd), min_seconds=0.3, tail_seconds=0)
        await rec.start()
        sd.streams[-1].feed(tone(1000))  # 0.06 s: a tap
        assert await rec.stop() is None
        await rec.start()
        sd.streams[-1].feed(tone(16000))
        wav = await rec.stop()
        assert wav is not None and wav[:4] == b"RIFF"

    asyncio.run(go())


def test_missing_sounddevice_says_how_to_fix_it(monkeypatch):
    monkeypatch.setitem(sys.modules, "sounddevice", None)  # makes `import sounddevice` raise ImportError
    with pytest.raises(RecorderError, match="windows' extra"):
        asyncio.run(SoundDeviceSource().start())


def test_resampler_preserves_duration_and_shape():
    src = tone(4800)
    for rate, n_in in ((48000, 4800), (44100, 4410), (8000, 800), (22050, 2205)):
        out = resample_s16(tone(n_in), rate)
        assert abs(len(out) // 2 - round(n_in * 16000 / rate)) <= 1
    assert resample_s16(src, 16000) == src and resample_s16(b"", 48000) == b""
    ramp = array.array("h", range(0, 4800)).tobytes()
    out = array.array("h")
    out.frombytes(resample_s16(ramp, 48000))
    assert out[0] == 0 and all(out[i] <= out[i + 1] for i in range(len(out) - 1))  # monotonic in, monotonic out
    assert out[10] == 30  # 3 source samples per output sample
