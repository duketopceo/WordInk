"""macOS microphone source against a fake `sounddevice` (plan M2)."""

from __future__ import annotations

import array
import asyncio
import sys

import pytest

from test_win_audio import fake_sd, tone
from wordink.desktop.interfaces import AudioSource
from wordink.desktop.mac.audio import MIN_SILENT_SECONDS, SoundDeviceSource, resample_s16
from wordink.desktop.recorder import Recorder, RecorderError


def zeros(n: int) -> bytes:
    return array.array("h", [0] * n).tobytes()


def test_it_is_an_audio_source_and_chunks_arrive_in_order():
    sd = fake_sd(default_rate=16000)
    src = SoundDeviceSource(sd)
    assert isinstance(src, AudioSource) and not src.active

    async def go():
        await src.start()
        assert src.active
        a, b = tone(1600), tone(800)
        sd.streams[-1].feed(a)
        sd.streams[-1].feed(b)
        assert src.captured_bytes == len(a) + len(b)
        return await src.read_all(), a + b

    got, want = asyncio.run(go())
    assert got == want and not src.active


def test_a_device_that_refuses_16k_is_resampled_from_its_own_rate():
    sd = fake_sd(refuse_rates=(16000,), default_rate=48000)
    src = SoundDeviceSource(sd)

    async def go():
        await src.start()
        assert sd.streams[-1].rate == 48000
        sd.streams[-1].feed(tone(4800))
        return await src.read_all()

    assert len(asyncio.run(go())) // 2 == 1600


@pytest.mark.parametrize(
    ("error", "needle"),
    [
        ("Error querying device -1: No Default Input Device Available", "System Settings, Sound, Input"),
        ("Error opening RawInputStream: Permission denied [PaErrorCode -9985]", "Privacy & Security, Microphone"),
        ("Something odd", "cannot open the microphone"),
    ],
)
def test_open_errors_point_at_system_settings(error, needle):
    src = SoundDeviceSource(fake_sd(fail_open=error, default_rate=16000))
    with pytest.raises(RecorderError, match=needle):
        asyncio.run(src.start())
    assert not src.active


def test_a_silent_capture_is_reported_as_a_probable_permission_block():
    sd = fake_sd(default_rate=16000)
    src = SoundDeviceSource(sd)

    async def go():
        await src.start()
        sd.streams[-1].feed(zeros(int(16000 * (MIN_SILENT_SECONDS + 0.1))))
        return await src.read_all()

    with pytest.raises(RecorderError, match="only silence.*Privacy & Security"):
        asyncio.run(go())
    assert not src.active


def test_a_short_silent_capture_is_not_flagged_and_real_sound_never_is():
    sd = fake_sd(default_rate=16000)
    src = SoundDeviceSource(sd)

    async def short():
        await src.start()
        sd.streams[-1].feed(zeros(int(16000 * (MIN_SILENT_SECONDS - 0.2))))
        return await src.read_all()

    assert len(asyncio.run(short())) == int(16000 * (MIN_SILENT_SECONDS - 0.2)) * 2

    async def sound():
        await src.start()
        sd.streams[-1].feed(zeros(16000) + tone(10))  # one non-zero sample anywhere is real audio
        return await src.read_all()

    assert len(asyncio.run(sound())) == (16000 + 10) * 2


def test_silence_is_measured_at_the_devices_own_rate():
    sd = fake_sd(refuse_rates=(16000,), default_rate=48000)
    src = SoundDeviceSource(sd)

    async def go():
        await src.start()
        sd.streams[-1].feed(zeros(int(48000 * 0.6)))  # 0.6 s at 48 kHz
        return await src.read_all()

    with pytest.raises(RecorderError, match="silence"):
        asyncio.run(go())


def test_a_failed_start_and_abort_leave_the_source_reusable():
    sd = fake_sd(fail_open="Permission denied")
    src = SoundDeviceSource(sd)
    with pytest.raises(RecorderError):
        asyncio.run(src.start())
    sd.fail_open = None

    async def go():
        await src.start()
        sd.streams[-1].feed(tone(100))
        await src.abort()
        assert not src.active and src.captured_bytes == 0
        await src.start()
        sd.streams[-1].feed(tone(50))
        return await src.read_all()

    assert len(asyncio.run(go())) == 100


def test_the_recorder_runs_on_it_and_a_silent_capture_surfaces_as_an_error():
    sd = fake_sd(default_rate=16000)

    async def go():
        rec = Recorder(source=SoundDeviceSource(sd), min_seconds=0.3, tail_seconds=0)
        await rec.start()
        sd.streams[-1].feed(tone(1000))  # 0.06 s: a tap
        assert await rec.stop() is None
        await rec.start()
        sd.streams[-1].feed(tone(16000))
        wav = await rec.stop()
        assert wav is not None and wav[:4] == b"RIFF"
        await rec.start()
        sd.streams[-1].feed(zeros(16000))
        await rec.stop()

    with pytest.raises(RecorderError, match="only silence"):
        asyncio.run(go())


def test_missing_sounddevice_names_the_macos_extra(monkeypatch):
    monkeypatch.setitem(sys.modules, "sounddevice", None)
    with pytest.raises(RecorderError, match="macos' extra"):
        asyncio.run(SoundDeviceSource().start())


def test_the_resampler_is_the_shared_one():
    from wordink.desktop import sdaudio

    assert resample_s16 is sdaudio.resample_s16
