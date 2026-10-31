"""The macOS app end to end on Linux: fake hotkeys, audio, menu bar and inserter; the real Unix socket (plan M6)."""

from __future__ import annotations

import asyncio
import json
import logging
import threading
from types import SimpleNamespace

import httpx
import pytest

from wordink.client import WordInkError
from wordink.desktop.config import DictateConfig
from wordink.desktop.inject import InsertResult
from wordink.desktop.mac.app import Adapters, MacApp, log_path, run_app, run_with_menu, setup_logging
from wordink.desktop.mac.hotkey import HotkeyClash
from wordink.desktop.mac.menubar import MenuBar

from dictd_helpers import short_tmp, wait_until

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
        self.start_thread: threading.Thread | None = None
        self.stop_thread: threading.Thread | None = None

    def start(self, press, release, toggle):
        self.start_thread = threading.current_thread()
        if self.fail:
            raise self.fail
        self.handlers = (press, release, toggle)

    def stop(self):
        self.stop_thread = threading.current_thread()
        self.stopped = True


class FakeInserter:
    def __init__(self) -> None:
        self.texts: list[str] = []

    def insert(self, text):
        self.texts.append(text)
        return InsertResult("typed", "fake")


class FakeRumps:
    """Just enough `rumps` to start a MenuBar; `run` returns when `quit_application` is called."""

    def __init__(self) -> None:
        self.notifications: list[str] = []
        self.quit = threading.Event()
        self.ran = threading.Event()
        self.titles: list[str] = []
        self.timers: list = []
        outer = self

        class App:
            def __init__(self, name, title, menu, quit_button):
                self.title = title

            def run(self):
                for tick in list(outer.timers):  # the Cocoa timer firing on the main thread
                    tick()
                outer.ran.set()
                outer.quit.wait(5)

        class Timer:
            def __init__(self, fn, interval):
                self.fn = fn

            def start(self):
                outer.timers.append(self.fn)

        self.App, self.Timer = App, Timer

    def MenuItem(self, title, callback=None):  # noqa: N802
        return SimpleNamespace(title=title, callback=callback, set_callback=lambda cb: None)

    def notification(self, title, sub, message):
        self.notifications.append(message)

    def quit_application(self):
        self.quit.set()


def groq():
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path.endswith("/audio/transcriptions"):
            return httpx.Response(200, json={"text": "um so ship it on the mac today"})
        return httpx.Response(200, json={"choices": [{"message": {"content": "Ship it on the Mac today."}}]})

    return httpx.MockTransport(handler)


def make(hotkeys=None):
    root = short_tmp()
    cfg = DictateConfig(
        runtime_dir=root / "run", vocab_path=root / "support" / "vocabulary.txt",
        failed_dir=root / "failed", min_seconds=0.2, tail_seconds=0.0,
    )  # fmt: skip
    rumps = FakeRumps()
    bar = MenuBar(rumps=rumps)
    ins, clip, opened = FakeInserter(), [], []
    ad = Adapters(
        audio=FakeAudio(), inserter=ins, hotkeys=hotkeys if hotkeys is not None else FakeHotkeys(),
        menubar=bar, clipboard_set=clip.append, open_path=opened.append,
    )  # fmt: skip
    return cfg, ad, ins, bar, rumps, clip, opened


def test_hold_to_talk_end_to_end():
    cfg, ad, ins, bar, _, _, _ = make()

    async def go():
        app = MacApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        app.start_hotkeys()
        try:
            press, release, _toggle = ad.hotkeys.handlers
            await asyncio.to_thread(press)  # the hotkey thread
            await wait_until(lambda: app.daemon.state == "recording")
            bar.drain()
            assert bar.state == "recording"
            await asyncio.to_thread(release)
            await wait_until(lambda: ins.texts)
            assert ins.texts == ["Ship it on the Mac today."]
            await wait_until(lambda: app.daemon.state == "idle")
            assert cfg.last_path.read_text() == "Ship it on the Mac today."
            assert (await app.daemon.transport.request("status")) == {"ok": True, "state": "idle"}
        finally:
            app.stop_hotkeys()
            await app.close()
        assert ad.hotkeys.stopped and not cfg.socket_path.exists()

    asyncio.run(go())


def test_starting_the_daemon_leaves_the_hotkeys_to_the_main_thread():
    cfg, ad, *_ = make()

    async def go():
        app = MacApp(cfg, ad, {}, http=groq())
        await app.start()  # in the real app this runs on the daemon's worker thread
        try:
            assert ad.hotkeys.start_thread is None and not app.hotkeys_active
        finally:
            await app.close()
        assert ad.hotkeys.stop_thread is None

    asyncio.run(go())


def test_the_socket_and_the_toggle_chord_both_drive_the_daemon():
    cfg, ad, ins, _, _, _, _ = make()

    async def go():
        app = MacApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        app.start_hotkeys()
        try:
            assert (await app.daemon.transport.request("toggle"))["ok"]
            await wait_until(lambda: app.daemon.state == "recording")
            await asyncio.to_thread(ad.hotkeys.handlers[2])  # the toggle chord stops it
            await wait_until(lambda: ins.texts)
        finally:
            await app.close()

    asyncio.run(go())


def test_a_second_instance_is_refused():
    cfg, ad, *_ = make()
    _, ad2, *_ = make()

    async def go():
        first = MacApp(cfg, ad, {}, http=groq())
        await first.start()
        try:
            with pytest.raises(WordInkError, match="already running"):
                await MacApp(cfg, ad2, {}, http=groq()).start()
        finally:
            await first.close()

    asyncio.run(go())


def test_a_hotkey_clash_leaves_the_menu_and_the_socket_working_and_says_so():
    cfg, ad, _, bar, rumps, _, _ = make(FakeHotkeys(HotkeyClash("hotkey Ctrl+Option+D is already used")))

    async def go():
        app = MacApp(cfg, ad, {}, http=groq())
        await app.start()
        app.start_hotkeys()
        try:
            assert not app.hotkeys_active
            bar.drain()
            assert bar.state == "error" and any("already used" in n for n in rumps.notifications)
            assert (await app.daemon.transport.request("status"))["ok"]
        finally:
            await app.close()
        assert not ad.hotkeys.stopped  # never started, so never stopped

    asyncio.run(go())


def test_the_menu_actions_reach_the_daemon_the_clipboard_and_the_folders(monkeypatch, tmp_path):
    monkeypatch.setenv("HOME", str(tmp_path))
    cfg, ad, ins, bar, _, clip, opened = make()

    async def go():
        app = MacApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        try:
            await asyncio.to_thread(bar.actions.toggle)
            await wait_until(lambda: app.daemon.state == "recording")
            await asyncio.to_thread(bar.actions.toggle)
            await wait_until(lambda: ins.texts)
            await asyncio.to_thread(bar.actions.copy_last)
            assert clip == ["Ship it on the Mac today."]
            bar.actions.open_config()
            bar.actions.open_log()
            assert opened == [cfg.vocab_path.parent, tmp_path / "Library" / "Logs" / "wordink" / "dictd.log"]
            await asyncio.to_thread(bar.actions.quit)
            await asyncio.wait_for(app.stop_event.wait(), 5)
        finally:
            await app.close()

    asyncio.run(go())


def test_copy_last_before_any_dictation_is_quiet():
    cfg, ad, _, bar, _, clip, _ = make()

    async def go():
        app = MacApp(cfg, ad, {}, http=groq())
        await app.start()
        try:
            await asyncio.to_thread(bar.actions.copy_last)
        finally:
            await app.close()

    asyncio.run(go())
    assert clip == []


def test_a_transcript_survives_an_inserter_that_fails():
    cfg, ad, ins, _, _, _, _ = make()

    def boom(text):
        raise OSError("blocked")

    ins.insert = boom

    async def go():
        app = MacApp(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
        await app.start()
        app.start_hotkeys()
        try:
            await asyncio.to_thread(ad.hotkeys.handlers[0])
            await wait_until(lambda: app.daemon.state == "recording")
            await asyncio.to_thread(ad.hotkeys.handlers[1])
            await wait_until(lambda: cfg.last_path.exists())
            assert cfg.last_path.read_text() == "Ship it on the Mac today."  # saved before the insert was tried
        finally:
            await app.close()

    asyncio.run(go())


def test_the_menu_bar_runs_on_the_main_thread_and_quit_ends_everything():
    cfg, ad, ins, bar, rumps, _, _ = make()
    code = {}

    def drive():
        assert rumps.ran.wait(5)
        ad.hotkeys.handlers[0]()  # a hotkey press, as the Cocoa loop delivers it
        deadline = threading.Event()
        for _ in range(500):
            if bar._queue.qsize() or bar.state == "recording":
                break
            deadline.wait(0.01)
        bar.actions.quit()  # Quit chosen in the menu

    t = threading.Thread(target=drive)
    t.start()
    code["rc"] = run_with_menu(cfg, ad, {"GROQ_API_KEY": "gsk_test"}, http=groq())
    t.join(5)
    assert code["rc"] == 0 and ad.hotkeys.stopped and not cfg.socket_path.exists()
    # registered inside the Cocoa loop and unregistered after it, both on the main thread
    assert ad.hotkeys.start_thread is threading.main_thread()
    assert ad.hotkeys.stop_thread is threading.main_thread()


def test_a_startup_failure_is_raised_and_the_menu_never_opens():
    cfg, ad, *_ = make()
    _, ad2, _, _, rumps2, _, _ = make()

    async def hold():
        first = MacApp(cfg, ad, {}, http=groq())
        await first.start()
        return first

    loop = asyncio.new_event_loop()
    first = loop.run_until_complete(hold())
    try:
        with pytest.raises(WordInkError, match="already running"):
            run_with_menu(cfg, ad2, {}, http=groq())
        assert not rumps2.ran.is_set()
    finally:
        loop.run_until_complete(first.close())
        loop.close()


def test_run_app_reports_the_app_to_the_caller():
    cfg, ad, *_ = make()
    seen = []

    async def go():
        task = asyncio.create_task(run_app(cfg, ad, {}, http=groq(), started=lambda app, exc: seen.append((app, exc))))
        await wait_until(lambda: seen)
        app, exc = seen[0]
        assert exc is None and isinstance(app, MacApp)
        app.request_stop()
        assert await task == 0

    asyncio.run(go())


def test_logs_go_to_the_library_logs_folder_without_secrets(tmp_path):
    env = {"HOME": str(tmp_path)}
    assert log_path(env) == tmp_path / "Library" / "Logs" / "wordink" / "dictd.log"
    root = logging.getLogger()
    before = list(root.handlers)
    try:
        setup_logging(False, env)
        logging.getLogger("wordink.dictd").info("hello")
        for h in root.handlers:
            h.flush()
        assert "hello" in log_path(env).read_text()
    finally:
        for h in list(root.handlers):
            if h not in before:
                root.removeHandler(h)
                h.close()


def test_a_missing_home_is_a_clear_error():
    with pytest.raises(WordInkError, match="HOME"):
        log_path({})


def test_no_native_framework_is_imported_outside_the_two_native_modules():
    import pathlib
    import re

    mac = pathlib.Path(__file__).parents[2] / "src" / "wordink" / "desktop" / "mac"
    pat = re.compile(r"^\s*(?:import|from)\s+(?:Quartz|AppKit|Cocoa|Foundation|AVFoundation|objc)\b", re.M)
    offenders = [p.name for p in mac.glob("*.py") if p.name not in ("quartz.py", "appkit.py") and pat.search(p.read_text())]
    assert offenders == []
