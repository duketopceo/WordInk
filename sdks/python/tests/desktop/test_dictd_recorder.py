"""The recorder against a real subprocess standing in for the microphone."""

from __future__ import annotations

import asyncio
import io
import wave

import pytest

from wordink.desktop.recorder import BACKENDS, Recorder, RecorderError, detect_backend, pcm_to_wav

from dictd_helpers import BROKEN_MIC, FAKE_MIC


def run(coro):
    return asyncio.run(coro)


def test_pcm_to_wav_is_16k_mono_s16():
    wav = pcm_to_wav(b"\x00\x01" * 100)
    with wave.open(io.BytesIO(wav)) as wf:
        assert (wf.getframerate(), wf.getnchannels(), wf.getsampwidth(), wf.getnframes()) == (16000, 1, 2, 100)


def test_detect_backend_prefers_pipewire_then_pulse_then_alsa():
    assert detect_backend(lambda n: "/usr/bin/" + n)[0] == "pw-record"
    assert detect_backend(lambda n: None if n == "pw-record" else "/x")[0] == "parecord"
    assert detect_backend(lambda n: "/x" if n == "arecord" else None)[0] == "arecord"


def test_detect_backend_without_any_tool_says_what_to_install():
    with pytest.raises(RecorderError, match="pw-record"):
        detect_backend(lambda n: None)


def test_every_backend_asks_for_raw_16k_mono_s16_on_stdout():
    for _, cmd in BACKENDS:
        joined = " ".join(cmd)
        assert "16000" in joined and ("s16" in joined.lower())
        assert "1" in cmd or "--channels=1" in cmd or "-c" in cmd


def test_records_and_returns_a_valid_wav():
    async def scenario():
        rec = Recorder(FAKE_MIC, min_seconds=0.2, tail_seconds=0.05)
        await rec.start()
        assert rec.recording
        await asyncio.sleep(0.5)
        wav = await rec.stop()
        assert not rec.recording
        return wav

    wav = run(scenario())
    assert wav is not None
    with wave.open(io.BytesIO(wav)) as wf:
        assert wf.getframerate() == 16000 and wf.getnchannels() == 1
        assert wf.getnframes() >= 16000 * 0.4


def test_a_quick_tap_is_discarded():
    async def scenario():
        rec = Recorder(FAKE_MIC, min_seconds=5, tail_seconds=0)
        await rec.start()
        await asyncio.sleep(0.2)
        return await rec.stop()

    assert run(scenario()) is None


def test_tail_keeps_listening_after_stop_so_the_last_word_is_not_clipped():
    async def both():
        rec = Recorder(FAKE_MIC, min_seconds=0, tail_seconds=0.4)
        await rec.start()
        await asyncio.sleep(0.3)
        before = rec.captured_seconds
        wav = await rec.stop()
        with wave.open(io.BytesIO(wav)) as wf:
            return before, wf.getnframes() / 16000

    before, total = run(both())
    assert total >= before + 0.25


def test_stop_ends_the_capture_process():
    async def scenario():
        rec = Recorder(FAKE_MIC, min_seconds=0, tail_seconds=0)
        await rec.start()
        proc = rec._proc
        await asyncio.sleep(0.2)
        await rec.stop()
        return proc.returncode

    assert run(scenario()) is not None


def test_a_dead_capture_tool_is_reported_not_swallowed():
    async def scenario():
        rec = Recorder(BROKEN_MIC, min_seconds=0.1, tail_seconds=0)
        await rec.start()
        await asyncio.sleep(0.5)
        await rec.stop()

    with pytest.raises(RecorderError, match="exited with code 3"):
        run(scenario())


def test_a_missing_tool_fails_to_start():
    async def scenario():
        await Recorder(("/nonexistent/mic-tool",)).start()

    with pytest.raises(RecorderError, match="cannot start"):
        run(scenario())


def test_starting_twice_is_an_error_and_abort_cleans_up():
    async def scenario():
        rec = Recorder(FAKE_MIC)
        await rec.start()
        with pytest.raises(RecorderError, match="already recording"):
            await rec.start()
        await rec.abort()
        assert not rec.recording
        return await rec.stop()

    assert run(scenario()) is None
