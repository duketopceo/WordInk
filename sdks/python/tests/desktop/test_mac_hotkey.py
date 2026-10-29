"""The macOS hotkey source against a fake `HotkeyApi` (plan M3)."""

from __future__ import annotations

import asyncio
import threading

import pytest

from wordink.client import WordInkError
from wordink.desktop.interfaces import HotkeySource as HotkeySourceProtocol
from wordink.desktop.mac.hotkey import HOLD_ID, TOGGLE_ID, HotkeyClash, HotkeySource, parse_chords
from wordink.desktop.mac.quartz import CarbonApi, HotkeyRegistrationError, carbon_modifiers

HOLD, TOGGLE = parse_chords("Ctrl+Option+D", "Ctrl+Option+S")


class FakeApi:
    def __init__(self, *, refuse: set[int] = frozenset(), clash: bool = True, run_error: Exception | None = None):
        self.refuse, self.clash, self.run_error = set(refuse), clash, run_error
        self.registered: dict[int, object] = {}
        self.register_calls: list[int] = []
        self.unregister_calls = 0
        self.stopped = threading.Event()
        self.handler = None
        self.run_thread: str | None = None

    def set_handler(self, handler):
        self.handler = handler

    def run(self, ready):
        self.run_thread = threading.current_thread().name
        if self.run_error:
            raise self.run_error
        ready()
        self.stopped.wait(5)

    def register(self, hotkey_id, chord):
        self.register_calls.append(hotkey_id)
        if hotkey_id in self.refuse:
            raise HotkeyRegistrationError("no", clash=self.clash)
        self.registered[hotkey_id] = chord

    def unregister_all(self):
        self.unregister_calls += 1
        self.registered.clear()

    def stop(self):
        self.stopped.set()

    def press(self, hotkey_id):
        self.handler(hotkey_id, True)

    def release(self, hotkey_id):
        self.handler(hotkey_id, False)


class Clock:
    def __init__(self):
        self.mono, self.skew = 1000.0, 5_000_000.0

    def wall(self):
        return self.mono + self.skew

    def sleep(self, seconds):  # wall time moves, monotonic time does not
        self.skew += seconds


def make(api=None, **kw):
    api = api or FakeApi()
    events: list[str] = []
    src = HotkeySource(HOLD, TOGGLE, api, wake_poll_interval=3600, **kw)
    src.start(lambda: events.append("press"), lambda: events.append("release"), lambda: events.append("toggle"))
    return src, api, events


def test_it_is_a_hotkey_source_and_registers_both_chords_on_start():
    src, api, _ = make()
    try:
        assert isinstance(src, HotkeySourceProtocol)
        assert api.registered == {HOLD_ID: HOLD, TOGGLE_ID: TOGGLE}
        assert api.run_thread == "wordink-hotkeys"
    finally:
        src.stop()


def test_press_then_release_in_order_and_toggle_press_only():
    src, api, events = make()
    try:
        api.press(HOLD_ID)
        api.release(HOLD_ID)
        api.press(TOGGLE_ID)
        api.release(TOGGLE_ID)
        api.press(TOGGLE_ID)
        assert events == ["press", "release", "toggle", "toggle"]
    finally:
        src.stop()


def test_key_repeat_and_a_stray_release_are_ignored():
    src, api, events = make()
    try:
        api.release(HOLD_ID)
        api.press(HOLD_ID)
        api.press(HOLD_ID)
        api.press(HOLD_ID)
        api.release(HOLD_ID)
        api.release(HOLD_ID)
        assert events == ["press", "release"]
    finally:
        src.stop()


def test_a_refused_chord_raises_a_clash_naming_it_and_leaves_nothing_registered():
    api = FakeApi(refuse={TOGGLE_ID})
    src = HotkeySource(HOLD, TOGGLE, api)
    with pytest.raises(HotkeyClash, match=r"Ctrl\+Option\+S.*already used") as err:
        src.start(lambda: None, lambda: None, lambda: None)
    assert isinstance(err.value, WordInkError)
    assert api.registered == {} and api.stopped.is_set()


def test_a_refusal_that_is_not_a_clash_says_so():
    api = FakeApi(refuse={HOLD_ID}, clash=False)
    with pytest.raises(HotkeyClash, match="could not be registered"):
        HotkeySource(HOLD, TOGGLE, api).start(lambda: None, lambda: None, lambda: None)


def test_an_event_loop_that_fails_to_start_is_a_clash_error_not_a_hang():
    api = FakeApi(run_error=OSError("no loop"))
    with pytest.raises(HotkeyClash, match="OSError"):
        HotkeySource(HOLD, TOGGLE, api).start(lambda: None, lambda: None, lambda: None)


def test_stop_unregisters_stops_the_loop_and_is_repeatable():
    src, api, _ = make()
    src.stop()
    assert api.registered == {} and api.stopped.is_set()
    src.stop()


def test_a_second_start_is_refused():
    src, _, _ = make()
    try:
        with pytest.raises(WordInkError, match="already running"):
            src.start(lambda: None, lambda: None, lambda: None)
    finally:
        src.stop()


def test_a_handler_that_raises_does_not_break_later_events():
    api = FakeApi()
    src = HotkeySource(HOLD, TOGGLE, api)
    seen: list[str] = []

    def boom():
        raise RuntimeError("daemon bug")

    src.start(boom, lambda: seen.append("release"), lambda: seen.append("toggle"))
    try:
        api.press(HOLD_ID)
        api.release(HOLD_ID)
        api.press(TOGGLE_ID)
        assert seen == ["release", "toggle"]
    finally:
        src.stop()


def test_callbacks_cross_threads_through_call_soon_threadsafe():
    async def go():
        loop = asyncio.get_running_loop()
        api = FakeApi()
        seen: list[tuple[str, str]] = []
        src = HotkeySource(HOLD, TOGGLE, api, loop=loop)
        main = threading.current_thread().name
        src.start(
            lambda: seen.append(("press", threading.current_thread().name)),
            lambda: seen.append(("release", threading.current_thread().name)),
            lambda: None,
        )
        try:
            t = threading.Thread(target=lambda: (api.press(HOLD_ID), api.release(HOLD_ID)))
            t.start()
            t.join()
            assert seen == []  # nothing ran on the API thread
            await asyncio.sleep(0.05)
            return seen, main
        finally:
            src.stop()

    seen, main = asyncio.run(go())
    assert seen == [("press", main), ("release", main)]


def test_wake_reregisters_both_chords():
    clock = Clock()
    src, api, events = make(wall=clock.wall, mono=lambda: clock.mono)
    try:
        assert src.check_wake() is False
        clock.sleep(2)  # a short stall is not a sleep
        assert src.check_wake() is False
        before = len(api.register_calls)
        clock.sleep(3600)
        api.registered.clear()  # the OS dropped them
        assert src.check_wake() is True
        assert api.registered == {HOLD_ID: HOLD, TOGGLE_ID: TOGGLE}
        assert len(api.register_calls) == before + 2 and src.reregistrations == 1
        assert src.check_wake() is False
        assert events == []
    finally:
        src.stop()


def test_wake_during_a_hold_releases_it_so_the_microphone_stops():
    clock = Clock()
    src, api, events = make(wall=clock.wall, mono=lambda: clock.mono)
    try:
        api.press(HOLD_ID)
        clock.sleep(600)
        assert src.check_wake() is True
        assert events == ["press", "release"]
        api.press(HOLD_ID)  # a fresh press works afterwards
        assert events == ["press", "release", "press"]
    finally:
        src.stop()


def test_wake_when_a_chord_was_taken_meanwhile_logs_and_keeps_running(caplog):
    clock = Clock()
    src, api, _ = make(wall=clock.wall, mono=lambda: clock.mono)
    try:
        api.refuse.add(HOLD_ID)
        clock.sleep(600)
        assert src.check_wake() is True
        assert src.reregistrations == 0 and "already used" in caplog.text
    finally:
        src.stop()


def test_carbon_modifier_mask():
    assert carbon_modifiers(HOLD) == 0x1000 | 0x800
    assert carbon_modifiers(parse_chords("Cmd+Shift+F5", "Cmd+Shift+F6")[0]) == 0x100 | 0x200


def test_the_carbon_api_imports_everywhere_but_refuses_to_run_off_a_mac():
    api = CarbonApi()
    api.stop()  # nothing running: a no-op
    api.unregister_all()
    import sys

    if sys.platform != "darwin":
        with pytest.raises(HotkeyRegistrationError, match="only on macOS"):
            api.register(HOLD_ID, HOLD)
