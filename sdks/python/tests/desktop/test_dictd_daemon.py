"""The daemon end to end: a real Unix socket, a fake microphone process, mocked Groq HTTP."""

from __future__ import annotations

import asyncio
import dataclasses
import json
import os
import stat
import threading
from pathlib import Path

import httpx
import pytest

from wordink.client import WordInkError
from wordink.desktop.config import DictateConfig
from wordink.desktop.daemon import Daemon, LastTranscriptSink, PipelineResult, build_pipeline, send_command
from wordink.desktop.indicator import StateFile
from wordink.desktop.recorder import Recorder
from wordink.desktop.vocab import add_terms

from dictd_helpers import BROKEN_MIC, FAKE_MIC, captured, short_tmp, wait_for_state, wait_until

KEY = "gsk_test_secret_key"
ENV = {"GROQ_API_KEY": KEY}


class SpyState(StateFile):
    def __init__(self, path: Path) -> None:
        super().__init__(path)
        self.seen: list[str] = []

    def write(self, state: str, detail: str | None = None) -> None:
        self.seen.append(state)
        super().write(state, detail)


def make_cfg(root: Path, **kw) -> DictateConfig:
    return DictateConfig(
        runtime_dir=root / "run",
        vocab_path=root / "vocabulary.txt",
        failed_dir=root / "failed",
        record_command=FAKE_MIC,
        min_seconds=0.2,
        tail_seconds=0.05,
        **kw,
    )


def groq(transcript: str = "um so i think we should uh ship wordink on hyprland tomorrow", cleaned: str | None = None, calls=None):
    cleaned = cleaned or "I think we should ship wordink on hyprland tomorrow."

    def handler(request: httpx.Request) -> httpx.Response:
        if calls is not None:
            calls.append(request.url.path)
        if request.url.path.endswith("/audio/transcriptions"):
            return httpx.Response(200, json={"text": transcript})
        return httpx.Response(200, json={"choices": [{"message": {"content": cleaned}}]})

    return httpx.MockTransport(handler)


async def started(cfg: DictateConfig, pipeline, sink=None, spy=None):
    daemon = Daemon(
        cfg,
        pipeline,
        sink or LastTranscriptSink(cfg.last_path),
        recorder=Recorder(cfg.record_command, min_seconds=cfg.min_seconds, tail_seconds=cfg.tail_seconds),
        indicator=spy or SpyState(cfg.state_path),
    )
    await daemon.serve()
    return daemon


async def cmd(cfg: DictateConfig, name: str) -> dict:
    return await send_command(cfg.socket_path, name)


def test_hold_to_talk_round_trip_writes_cleaned_text_and_walks_the_states():
    root = short_tmp()
    cfg = make_cfg(root)
    add_terms(cfg.vocab_path, ["WordInk", "Hyprland"])
    calls: list[str] = []

    async def scenario():
        spy = SpyState(cfg.state_path)
        daemon = await started(cfg, build_pipeline(cfg, ENV, transport=groq(calls=calls)), spy=spy)
        try:
            assert (await cmd(cfg, "start"))["state"] == "recording"
            assert StateFile(cfg.state_path).read()["state"] == "recording"
            await captured(daemon)
            assert (await cmd(cfg, "stop"))["state"] == "transcribing"
            await daemon.wait_idle()
            assert (await cmd(cfg, "status"))["state"] == "idle"
            return spy.seen
        finally:
            await daemon.close()

    seen = asyncio.run(scenario())
    assert seen == ["idle", "recording", "transcribing", "idle"]
    assert cfg.last_path.read_text() == "I think we should ship WordInk on Hyprland tomorrow."
    assert stat.S_IMODE(cfg.last_path.stat().st_mode) == 0o600
    assert calls == ["/openai/v1/audio/transcriptions", "/openai/v1/chat/completions"]
    assert not cfg.socket_path.exists()


def test_the_transcription_request_carries_the_vocabulary_hint():
    root = short_tmp()
    cfg = make_cfg(root)
    add_terms(cfg.vocab_path, ["Omarchy"])
    bodies: list[bytes] = []

    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path.endswith("/audio/transcriptions"):
            bodies.append(request.content)
            return httpx.Response(200, json={"text": "hello"})
        return httpx.Response(200, json={"choices": [{"message": {"content": "Hello."}}]})

    async def scenario():
        daemon = await started(cfg, build_pipeline(cfg, ENV, transport=httpx.MockTransport(handler)))
        try:
            await cmd(cfg, "start")
            await captured(daemon)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
        finally:
            await daemon.close()

    asyncio.run(scenario())
    assert b"Omarchy" in bodies[0] and b"RIFF" in bodies[0]


def test_double_start_stop_while_idle_and_toggle():
    root = short_tmp()
    cfg = make_cfg(root)

    async def scenario():
        daemon = await started(cfg, lambda wav: PipelineResult("text", False, "x"))
        try:
            assert (await cmd(cfg, "stop"))["note"] == "not recording"
            assert (await cmd(cfg, "toggle"))["state"] == "recording"
            assert (await cmd(cfg, "start"))["note"] == "already recording"
            await captured(daemon)
            assert (await cmd(cfg, "toggle"))["state"] == "transcribing"
            await daemon.wait_idle()
            return (await cmd(cfg, "status"))["state"]
        finally:
            await daemon.close()

    assert asyncio.run(scenario()) == "idle"
    assert cfg.last_path.read_text() == "text"


def test_a_quick_tap_is_discarded_without_calling_groq():
    root = short_tmp()
    cfg = make_cfg(root)
    cfg = dataclasses.replace(cfg, min_seconds=10.0)
    calls: list[str] = []

    async def scenario():
        daemon = await started(cfg, build_pipeline(cfg, ENV, transport=groq(calls=calls)))
        try:
            await cmd(cfg, "start")
            await captured(daemon, 0.1)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
            return StateFile(cfg.state_path).read()
        finally:
            await daemon.close()

    state = asyncio.run(scenario())
    assert state["state"] == "idle" and "too short" in state["detail"]
    assert calls == [] and not cfg.last_path.exists()


def test_start_is_refused_while_transcribing():
    root = short_tmp()
    cfg = make_cfg(root)
    gate = threading.Event()

    def slow(wav: bytes) -> PipelineResult:
        assert gate.wait(10), 'test never released the pipeline'
        return PipelineResult("done", False, "x")

    async def scenario():
        daemon = await started(cfg, slow)
        try:
            await cmd(cfg, "start")
            await captured(daemon)
            await cmd(cfg, "stop")
            busy = await cmd(cfg, "start")
            gate.set()
            await daemon.wait_idle()
            return busy
        finally:
            gate.set()
            await daemon.close()

    busy = asyncio.run(scenario())
    assert busy["ok"] is False and "transcribing" in busy["error"]


def test_transcription_failure_sets_error_keeps_audio_and_leaks_no_key():
    root = short_tmp()
    cfg = make_cfg(root)
    down = httpx.MockTransport(lambda r: httpx.Response(401, json={"error": {"message": f"bad key {KEY}"}}))

    async def scenario():
        daemon = await started(cfg, build_pipeline(cfg, ENV, transport=down))
        try:
            await cmd(cfg, "start")
            await captured(daemon)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
            after = await cmd(cfg, "status")
            state_text = cfg.state_path.read_text()
            again = await cmd(cfg, "start")  # an error does not wedge the daemon
            await cmd(cfg, "stop")
            await daemon.wait_idle()
            return after, again, state_text
        finally:
            await daemon.close()

    after, again, state_text = asyncio.run(scenario())
    assert after["state"] == "error" and again["state"] == "recording"
    assert KEY not in state_text and "bad key" in state_text and "audio kept at" in state_text
    kept = list(cfg.failed_dir.glob("dictation-*.wav"))
    assert kept and kept[0].read_bytes().startswith(b"RIFF")


def test_a_missing_groq_key_surfaces_an_actionable_error():
    root = short_tmp()
    cfg = make_cfg(root)

    async def scenario():
        daemon = await started(cfg, build_pipeline(cfg, {}, transport=groq()))
        try:
            await cmd(cfg, "start")
            await captured(daemon)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
            return StateFile(cfg.state_path).read()
        finally:
            await daemon.close()

    state = asyncio.run(scenario())
    assert state["state"] == "error" and "GROQ_API_KEY is not set" in state["detail"]


def test_a_broken_microphone_reports_an_error_state():
    root = short_tmp()
    cfg = dataclasses.replace(make_cfg(root), record_command=BROKEN_MIC)

    async def scenario():
        daemon = await started(cfg, lambda wav: PipelineResult("x", False, "x"))
        try:
            await cmd(cfg, "start")
            await wait_until(lambda: daemon.recorder._source._proc is not None and daemon.recorder._source._proc.returncode is not None)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
            return StateFile(cfg.state_path).read()
        finally:
            await daemon.close()

    state = asyncio.run(scenario())
    assert state["state"] == "error" and "exited with code 3" in state["detail"]


def test_max_duration_stops_the_recording_on_its_own():
    root = short_tmp()
    cfg = dataclasses.replace(make_cfg(root), max_seconds=1.0)

    async def scenario():
        daemon = await started(cfg, lambda wav: PipelineResult("auto stopped", False, "x"))
        try:
            await cmd(cfg, "start")
            await wait_until(lambda: daemon.state == "transcribing" or daemon.state == "idle" and daemon._job is not None)
            await daemon.wait_idle()
            return (await cmd(cfg, "status"))["state"]
        finally:
            await daemon.close()

    assert asyncio.run(scenario()) == "idle"
    assert cfg.last_path.read_text() == "auto stopped"


def test_unknown_command_is_refused_and_socket_is_private():
    root = short_tmp()
    cfg = make_cfg(root)

    async def scenario():
        daemon = await started(cfg, lambda wav: PipelineResult("x", False, "x"))
        try:
            mode = stat.S_IMODE(os.stat(cfg.socket_path).st_mode)
            dirmode = stat.S_IMODE(os.stat(cfg.socket_path.parent).st_mode)
            return await cmd(cfg, "format-disk"), mode, dirmode
        finally:
            await daemon.close()

    reply, mode, dirmode = asyncio.run(scenario())
    assert reply["ok"] is False and "unknown command" in reply["error"]
    assert mode == 0o600 and dirmode == 0o700


def test_stale_socket_is_replaced_and_a_live_one_is_respected():
    root = short_tmp()
    cfg = make_cfg(root)
    cfg.runtime_dir.mkdir(parents=True)
    import socket as _socket

    dead = _socket.socket(_socket.AF_UNIX)
    dead.bind(str(cfg.socket_path))
    dead.close()  # leaves a socket file nobody listens on
    assert cfg.socket_path.exists()

    async def scenario():
        first = await started(cfg, lambda wav: PipelineResult("x", False, "x"))
        try:
            assert (await cmd(cfg, "status"))["state"] == "idle"
            with pytest.raises(WordInkError, match="already running"):
                await started(cfg, lambda wav: PipelineResult("x", False, "x"))
            assert (await cmd(cfg, "status"))["ok"] is True
        finally:
            await first.close()

    asyncio.run(scenario())


def test_send_command_without_a_daemon_says_how_to_start_one():
    root = short_tmp()
    with pytest.raises(WordInkError, match="wordink-dictd run"):
        asyncio.run(send_command(root / "none.sock", "status"))


def test_state_file_is_one_json_line(tmp_path):
    sf = StateFile(tmp_path / "s" / "state")
    sf.write("recording")
    text = (tmp_path / "s" / "state").read_text()
    assert text.endswith("\n") and json.loads(text)["state"] == "recording"
    with pytest.raises(ValueError):
        sf.write("exploding")
