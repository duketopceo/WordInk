"""The Windows app end to end on Linux: fake hotkeys, audio, tray and inserter; real loopback transport (plan W7b)."""

from __future__ import annotations

import asyncio
import json
import threading
import types
from pathlib import Path

import httpx
import pytest

from wordink.client import WordInkError
from wordink.desktop.config import DictateConfig
from wordink.desktop.inject import InsertResult
from wordink.desktop.win.app import Adapters, WinApp, log_path, run_app
from wordink.desktop.win.hook import HookUnavailable
from wordink.desktop.win.tray import Tray, TrayActions
from wordink.desktop.win.transport import LoopbackTransport

from dictd_helpers import wait_until

PCM = b"\x01\x00" * 16000  # 1 s


class FakeAudio:
    def __init__(self) -> None:
        self.on = False

    async def start(self):
        self.on = True

    async def read_all(self):
        self.on = False
        return PCM

    async def abort(self):
        self.on = False

    @property
    def active(self):
        return self.on

    @property
    def captured_bytes(self):
        return len(PCM) if self.on else 0


class FakeHotkeys:
    def __init__(self, fail: Exception | None = None) -> None:
        self.fail, self.handlers, self.stopped = fail, None, False

    def start(self, press, release, toggle):
        if self.fail:
            raise self.fail
        self.handlers = (press, release, toggle)

    def stop(self):
        self.stopped = True


class FakeInserter:
    def __init__(self) -> None:
        self.texts: list[str] = []

    def insert(self, text):
        self.texts.append(text)
        return InsertResult("typed", "fake")


class FakeIcon:
    def __init__(self, name, image, title, menu) -> None:
        self.icon, self.title, self.menu, self.toasts, self.running = image, title, menu, [], False

    def run_detached(self):
        self.running = True

    def stop(self):
        self.running = False

    def notify(self, message, title=None):
        self.toasts.append(message)


def fake_tray():
    ps = types.SimpleNamespace(
        MenuItem=lambda text, action=None: types.SimpleNamespace(text=text, action=action),
        Icon=FakeIcon,
        Menu=type("Menu", (), {"SEPARATOR": "-", "__init__": lambda self, *i: setattr(self, "items", i)}),
    )
    return Tray(TrayActions(*(lambda: None,) * 5), pystray=ps, make_image=lambda rgba, size: ("img", size))


def groq():
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path.endswith("/audio/transcriptions"):
            return httpx.Response(200, json={"text": "um so ship it on windows today"})
        return httpx.Response(200, json={"choices": [{"message": {"content": "Ship it on Windows today."}}]})

    return httpx.MockTransport(handler)


def make(tmp_path: Path, hotkeys=None):
    cfg = DictateConfig(
        runtime_dir=tmp_path / "local" / "run", vocab_path=tmp_path / "roaming" / "vocabulary.txt",
        failed_dir=tmp_path / "local" / "failed", min_seconds=0.2, tail_seconds=0.0,
    )
    ins, tray, clip, opened = FakeInserter(), fake_tray(), [], []
    ad = Adapters(
        audio=FakeAudio(), inserter=ins, hotkeys=hotkeys if hotkeys is not None else FakeHotkeys(),
        transport=LoopbackTransport(cfg.runtime_dir), tray=tray, clipboard_set=clip.append, open_path=opened.append,
    )
    return cfg, ad, ins, tray, clip, opened


def test_hold_to_talk_end_to_end(tmp_path):
    cfg, ad, ins, tray, _, _ = make(tmp_path)

    async def go():
        app = WinApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        try:
            press, release, _toggle = ad.hotkeys.handlers
            await asyncio.to_thread(press)  # the hotkey thread
            await wait_until(lambda: app.daemon.state == "recording")
            assert tray._icon.title.endswith("listening")
            await asyncio.to_thread(release)
            await wait_until(lambda: ins.texts)
            assert ins.texts == ["Ship it on Windows today."]
            await wait_until(lambda: app.daemon.state == "idle")
            assert cfg.last_path.read_text() == "Ship it on Windows today."
            reply = await ad.transport.request("status")
            assert reply == {"ok": True, "state": "idle"}
        finally:
            await app.close()
        assert ad.hotkeys.stopped and not tray._icon and not cfg.runtime_dir.joinpath("dictd.port").exists()

    asyncio.run(go())


def test_the_cli_channel_toggles_and_the_hotkey_toggle_works(tmp_path):
    cfg, ad, ins, _, _, _ = make(tmp_path)

    async def go():
        app = WinApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        try:
            assert (await ad.transport.request("toggle"))["ok"]
            await wait_until(lambda: app.daemon.state == "recording")
            await asyncio.to_thread(ad.hotkeys.handlers[2])  # toggle chord stops it
            await wait_until(lambda: ins.texts)
        finally:
            await app.close()

    asyncio.run(go())


def test_a_second_instance_is_refused(tmp_path):
    cfg, ad, *_ = make(tmp_path)
    cfg2, ad2, *_ = make(tmp_path)

    async def go():
        first = WinApp(cfg, ad, {}, http=groq())
        await first.start()
        try:
            with pytest.raises(WordInkError, match="already running"):
                await WinApp(cfg2, ad2, {}, http=groq()).start()
        finally:
            await first.close()

    asyncio.run(go())


def test_a_blocked_hook_leaves_the_tray_working_and_says_so(tmp_path):
    cfg, ad, _, tray, _, _ = make(tmp_path, FakeHotkeys(HookUnavailable("cannot install the keyboard hook")))

    async def go():
        app = WinApp(cfg, ad, {}, http=groq())
        await app.start()
        try:
            assert not app.hotkeys_active
            assert tray.state == "error" and "tray menu" in tray._icon.title
            assert (await ad.transport.request("status"))["ok"]  # still serving the CLI
        finally:
            await app.close()
        assert not ad.hotkeys.stopped  # never started, so never stopped

    asyncio.run(go())


def test_a_blocked_hook_falls_back_to_toggle_only_hotkeys(tmp_path):
    cfg, ad, ins, tray, _, _ = make(tmp_path, FakeHotkeys(HookUnavailable("blocked")))
    ad.fallback_hotkeys = FakeHotkeys()

    async def go():
        app = WinApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        try:
            assert app.hotkeys_active and tray.state == "idle"
            assert any("toggles recording" in t for t in tray._icon.toasts)
            press, release, toggle = ad.fallback_hotkeys.handlers
            press(), release()  # no hold semantics: these do nothing
            await asyncio.sleep(0.05)
            assert app.daemon.state == "idle"
            await asyncio.to_thread(toggle)
            await wait_until(lambda: app.daemon.state == "recording")
        finally:
            await app.close()
        assert ad.fallback_hotkeys.stopped

    asyncio.run(go())


def test_a_fallback_that_also_fails_leaves_the_tray_menu(tmp_path):
    cfg, ad, _, tray, _, _ = make(tmp_path, FakeHotkeys(HookUnavailable("blocked")))
    ad.fallback_hotkeys = FakeHotkeys(WordInkError("another application already uses Alt+Ctrl+D"))

    async def go():
        app = WinApp(cfg, ad, {}, http=groq())
        await app.start()
        try:
            assert not app.hotkeys_active and tray.state == "error"
        finally:
            await app.close()

    asyncio.run(go())


def test_a_transcription_failure_toasts_once_and_keeps_the_audio(tmp_path):
    cfg, ad, ins, tray, _, _ = make(tmp_path)
    boom = httpx.MockTransport(lambda r: httpx.Response(401, json={"error": {"message": "bad key"}}))

    async def go():
        app = WinApp(cfg, ad, {"GROQ_API_KEY": "gsk_wrong"}, http=boom)
        await app.start()
        try:
            await ad.transport.request("start")
            await wait_until(lambda: app.daemon.state == "recording")
            await ad.transport.request("stop")
            await wait_until(lambda: app.daemon.state == "error")
            assert len(tray._icon.toasts) == 1 and "gsk_wrong" not in tray._icon.toasts[0]
            assert ins.texts == []
            assert list((cfg.failed_dir).glob("*.wav"))
        finally:
            await app.close()

    asyncio.run(go())


def test_tray_menu_actions_reach_the_daemon_and_the_os(tmp_path):
    cfg, ad, ins, tray, clip, opened = make(tmp_path)
    cfg.last_path.parent.mkdir(parents=True, exist_ok=True)
    cfg.last_path.write_text("previous text")

    async def go():
        app = WinApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        try:
            items = {i.text: i.action for i in tray._icon.menu.items if hasattr(i, "text")}
            await asyncio.to_thread(items["Copy last transcript"], None, None)
            assert clip == ["previous text"]
            await asyncio.to_thread(items["Open config folder"], None, None)
            await asyncio.to_thread(items["Open log"], None, None)
            assert opened == [cfg.vocab_path.parent, log_path(cfg)]
            await asyncio.to_thread(items["Start / stop dictation"], None, None)
            await wait_until(lambda: app.daemon.state == "recording")
            await asyncio.to_thread(items["Quit WordInk"], None, None)
            await asyncio.wait_for(app.stop_event.wait(), 3)
        finally:
            await app.close()

    asyncio.run(go())


def test_run_app_returns_when_asked_to_stop(tmp_path):
    cfg, ad, *_ = make(tmp_path)

    async def go():
        task = asyncio.create_task(run_app(cfg, ad, {}, http=groq()))
        await wait_until(lambda: ad.transport.alive())
        await ad.transport.request("status")
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        assert not ad.transport.alive()  # the finally block closed everything

    asyncio.run(go())


def test_the_transcript_is_kept_even_when_insertion_fails(tmp_path):
    cfg, ad, ins, tray, _, _ = make(tmp_path)

    def broken(text):
        raise WordInkError("cannot insert text")

    ins.insert = broken

    async def go():
        app = WinApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        try:
            await ad.transport.request("start")
            await wait_until(lambda: app.daemon.state == "recording")
            await ad.transport.request("stop")
            await wait_until(lambda: app.daemon.state == "error")
            assert cfg.last_path.read_text() == "Ship it on Windows today."  # recoverable
        finally:
            await app.close()

    asyncio.run(go())
