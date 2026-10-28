"""The hook thread, dispatcher and watchdog against a fake HookApi (plan W4b)."""

from __future__ import annotations

import threading
import time

import pytest

from wordink.desktop.interfaces import HotkeySource
from wordink.desktop.win.hook import HEARTBEAT_VK, HookHotkeySource, HookUnavailable, probe_clashes
from wordink.desktop.win.hotkey import parse_chord, parse_chords

LCTRL, LALT, D, S, A = 0xA2, 0xA4, ord("D"), ord("S"), ord("A")


def wait_for(pred, timeout=3.0):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if pred():
            return True
        time.sleep(0.005)
    return False


class FakeHookApi:
    def __init__(self) -> None:
        self.on_key = None
        self.alive = True
        self.installs = 0
        self.fail_install: Exception | None = None
        self.clashes: set[str] = set()
        self._quit = threading.Event()
        self._running = threading.Event()

    def run_hook(self, on_key, ready):
        if self.fail_install:
            raise self.fail_install
        self.installs += 1
        self.on_key, self.alive = on_key, True
        self._quit.clear()
        self._running.set()
        ready()
        self._quit.wait()
        self._running.clear()

    def stop_hook(self):
        self._quit.set()

    def send_heartbeat(self):
        if self.alive and self.on_key:
            self.on_key(HEARTBEAT_VK, False, True)

    def probe_hotkey(self, chord):
        return chord.text not in self.clashes

    def key(self, vk, down=True, injected=False):
        return self.on_key(vk, down, injected)


def make(**kw):
    api = FakeHookApi()
    hold, toggle = parse_chords("Ctrl+Alt+D", "Ctrl+Alt+S")
    src = HookHotkeySource(hold, toggle, api, **kw)
    got: list[str] = []
    return api, src, got


def start(src, got):
    src.start(lambda: got.append("press"), lambda: got.append("release"), lambda: got.append("toggle"))


def test_it_is_a_hotkey_source_and_dispatches_hold_and_toggle():
    api, src, got = make(heartbeat_interval=60)
    assert isinstance(src, HotkeySource)
    start(src, got)
    try:
        api.key(LCTRL), api.key(LALT)
        assert api.key(D) is True  # swallowed
        assert wait_for(lambda: got == ["press"])
        assert api.key(D, False) is True
        assert wait_for(lambda: got == ["press", "release"])
        assert api.key(A) is False  # other keys pass
        api.key(S)
        assert wait_for(lambda: got[-1] == "toggle")
    finally:
        src.stop()


def test_the_callback_returns_immediately_even_when_the_handler_is_slow():
    api, src, _ = make(heartbeat_interval=60)
    gate = threading.Event()
    src.start(lambda: gate.wait(2), lambda: None, lambda: None)
    try:
        api.key(LCTRL), api.key(LALT)
        t0 = time.perf_counter()
        api.key(D)
        assert time.perf_counter() - t0 < 0.05  # the handler blocks on `gate`, the callback does not
    finally:
        gate.set()
        src.stop()


def test_a_handler_exception_does_not_kill_dispatch():
    api, src, _ = make(heartbeat_interval=60)
    seen: list[str] = []

    def boom():
        raise RuntimeError("daemon bug")

    src.start(boom, lambda: seen.append("release"), lambda: None)
    try:
        api.key(LCTRL), api.key(LALT), api.key(D)
        api.key(D, False)
        assert wait_for(lambda: seen == ["release"])
    finally:
        src.stop()


def test_injected_keys_never_trigger_it():
    api, src, got = make(heartbeat_interval=60)
    start(src, got)
    try:
        api.key(LCTRL, True, True), api.key(LALT, True, True), api.key(D, True, True)
        time.sleep(0.05)
        assert got == []
    finally:
        src.stop()


def test_watchdog_reinstalls_a_dropped_hook_and_releases_a_stuck_hold():
    api, src, got = make(heartbeat_interval=0.05, heartbeat_timeout=0.1)
    start(src, got)
    try:
        api.key(LCTRL), api.key(LALT), api.key(D)
        assert wait_for(lambda: got == ["press"])
        api.alive = False  # Windows silently dropped the hook; heartbeats stop arriving
        assert wait_for(lambda: src.reinstalls >= 1 and api.installs >= 2)
        assert wait_for(lambda: "release" in got)  # the microphone must not keep recording
        assert not src.tracker.holding
        # and the new hook works
        api.key(LCTRL), api.key(LALT), api.key(D)
        assert wait_for(lambda: got.count("press") == 2)
    finally:
        src.stop()


def test_a_healthy_hook_is_left_alone():
    api, src, got = make(heartbeat_interval=0.03, heartbeat_timeout=0.1)
    start(src, got)
    try:
        time.sleep(0.3)
        assert src.reinstalls == 0 and api.installs == 1
    finally:
        src.stop()


def test_a_blocked_hook_raises_hook_unavailable_without_leaving_threads():
    api, src, got = make()
    api.fail_install = OSError("blocked by policy")
    before = threading.active_count()
    with pytest.raises(HookUnavailable, match="toggle-only"):
        start(src, got)
    assert wait_for(lambda: threading.active_count() <= before)


def test_stop_ends_every_thread_and_is_safe_twice():
    api, src, got = make(heartbeat_interval=0.05)
    start(src, got)
    src.stop()
    src.stop()
    assert not any(t.name.startswith("wordink-") and t.is_alive() for t in threading.enumerate())


def test_probe_reports_chords_another_app_owns():
    api = FakeHookApi()
    api.clashes = {"Alt+Ctrl+S"}
    hold, toggle = parse_chord("Ctrl+Alt+D"), parse_chord("Ctrl+Alt+S")
    assert probe_clashes(api, hold, toggle) == ["Alt+Ctrl+S"]
    assert probe_clashes(api, hold) == []
