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
    """A `HotkeyApi` that records which thread each call came from."""

    def __init__(self, *, refuse: set[int] = frozenset(), clash: bool = True, install_error: Exception | None = None):
        self.refuse, self.clash, self.install_error = set(refuse), clash, install_error
        self.registered: dict[int, object] = {}
        self.register_calls: list[int] = []
        self.unregister_calls = 0
        self.installed = False
        self.uninstalled = False
        self.handler = None
        self.threads: dict[str, threading.Thread] = {}

    def set_handler(self, handler):
        self.handler = handler

    def install(self):
        self.threads["install"] = threading.current_thread()
        if self.install_error:
            raise self.install_error
        self.installed = True

    def register(self, hotkey_id, chord):
        self.threads["register"] = threading.current_thread()
        self.register_calls.append(hotkey_id)
        if hotkey_id in self.refuse:
            raise HotkeyRegistrationError("no", clash=self.clash)
        self.registered[hotkey_id] = chord

    def unregister_all(self):
        self.unregister_calls += 1
        self.registered.clear()

    def uninstall(self):
        self.uninstalled = True
        self.installed = False
        self.registered.clear()

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
    src = HotkeySource(HOLD, TOGGLE, api, **kw)
    src.start(lambda: events.append("press"), lambda: events.append("release"), lambda: events.append("toggle"))
    return src, api, events


def test_it_is_a_hotkey_source_and_registers_both_chords_on_start():
    src, api, _ = make()
    try:
        assert isinstance(src, HotkeySourceProtocol)
        assert api.registered == {HOLD_ID: HOLD, TOGGLE_ID: TOGGLE}
        assert api.installed
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
    assert api.registered == {} and api.uninstalled


def test_a_refusal_that_is_not_a_clash_says_so():
    api = FakeApi(refuse={HOLD_ID}, clash=False)
    with pytest.raises(HotkeyClash, match="could not be registered"):
        HotkeySource(HOLD, TOGGLE, api).start(lambda: None, lambda: None, lambda: None)


def test_a_handler_that_cannot_be_installed_is_a_clash_error_and_registers_nothing():
    api = FakeApi(install_error=HotkeyRegistrationError("InstallEventHandler failed (-50)"))
    with pytest.raises(HotkeyClash, match="InstallEventHandler"):
        HotkeySource(HOLD, TOGGLE, api).start(lambda: None, lambda: None, lambda: None)
    assert api.register_calls == [] and api.uninstalled


def test_stop_unregisters_uninstalls_and_is_repeatable():
    src, api, _ = make()
    src.stop()
    assert api.registered == {} and api.uninstalled and not api.installed
    src.stop()


def test_the_chords_are_registered_on_the_main_thread_and_no_event_loop_thread_is_started():
    before = {t.name for t in threading.enumerate()}
    src, api, _ = make()
    try:
        assert api.threads["install"] is threading.main_thread()
        assert api.threads["register"] is threading.main_thread()
        assert {t.name for t in threading.enumerate()} == before  # the Cocoa loop delivers events, not a thread
    finally:
        src.stop()


def test_starting_off_the_main_thread_is_refused_before_anything_is_registered():
    api = FakeApi()
    src = HotkeySource(HOLD, TOGGLE, api)
    errors: list[Exception] = []

    def worker():
        try:
            src.start(lambda: None, lambda: None, lambda: None)
        except WordInkError as exc:
            errors.append(exc)

    t = threading.Thread(target=worker, name="wordink-daemon")
    t.start()
    t.join()
    assert len(errors) == 1 and "main thread" in str(errors[0])
    assert api.threads == {} and api.registered == {} and not api.installed


def test_stopping_and_wake_checks_off_the_main_thread_are_refused():
    src, api, _ = make()
    try:
        errors: list[Exception] = []

        def worker():
            for call in (src.stop, src.check_wake):
                try:
                    call()
                except WordInkError as exc:
                    errors.append(exc)

        t = threading.Thread(target=worker)
        t.start()
        t.join()
        assert len(errors) == 2 and api.registered  # nothing was touched
    finally:
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
            assert seen == []  # nothing ran on the thread the event arrived on
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


def test_poll_checks_for_a_wake_only_once_per_interval_and_only_while_running():
    clock = Clock()
    src, api, _ = make(wall=clock.wall, mono=lambda: clock.mono, wake_poll_interval=5.0)
    try:
        clock.sleep(600)
        src.poll()  # too soon after start: not even looked at
        assert src.reregistrations == 0
        clock.mono += 6
        src.poll()
        assert src.reregistrations == 1
        clock.mono += 1
        clock.sleep(600)
        src.poll()  # inside the interval again
        assert src.reregistrations == 1
    finally:
        src.stop()
    clock.mono += 60
    clock.sleep(600)
    src.poll()  # stopped: never re-registers
    assert src.reregistrations == 1 and api.registered == {}


def test_carbon_modifier_mask():
    assert carbon_modifiers(HOLD) == 0x1000 | 0x800
    assert carbon_modifiers(parse_chords("Cmd+Shift+F5", "Cmd+Shift+F6")[0]) == 0x100 | 0x200


def test_the_carbon_api_imports_everywhere_but_refuses_to_run_off_a_mac():
    api = CarbonApi()
    api.uninstall()  # nothing installed: a no-op
    api.unregister_all()
    import sys

    if sys.platform != "darwin":
        with pytest.raises(HotkeyRegistrationError, match="only on macOS"):
            api.install()
        with pytest.raises(HotkeyRegistrationError, match="only on macOS"):
            api.register(HOLD_ID, HOLD)


class FakeCarbon:
    """Just the Carbon calls `CarbonApi` may make. Any other name (RunApplicationEventLoop, ...) raises."""

    def __init__(self, install_status: int = 0):
        self.calls: list[tuple[str, threading.Thread]] = []
        self.install_status = install_status

    def _note(self, name: str) -> None:
        self.calls.append((name, threading.current_thread()))

    def GetApplicationEventTarget(self):  # noqa: N802
        self._note("GetApplicationEventTarget")
        return 1

    def InstallEventHandler(self, *args):  # noqa: N802
        self._note("InstallEventHandler")
        return self.install_status

    def RegisterEventHotKey(self, *args):  # noqa: N802
        self._note("RegisterEventHotKey")
        return 0

    def UnregisterEventHotKey(self, ref):  # noqa: N802
        self._note("UnregisterEventHotKey")
        return 0

    def RemoveEventHandler(self, ref):  # noqa: N802
        self._note("RemoveEventHandler")
        return 0

    def names(self) -> list[str]:
        return [name for name, _ in self.calls]


def carbon_with(fake: FakeCarbon) -> CarbonApi:
    api = CarbonApi()
    api._lib = fake  # type: ignore[assignment]  # a fake framework in place of ctypes.CDLL
    return api


def test_carbon_installs_registers_and_uninstalls_on_the_calling_thread_without_an_event_loop():
    fake = FakeCarbon()
    src = HotkeySource(HOLD, TOGGLE, carbon_with(fake))
    src.start(lambda: None, lambda: None, lambda: None)
    src.stop()
    assert fake.names().count("InstallEventHandler") == 1
    assert fake.names().count("RegisterEventHotKey") == 2
    assert fake.names().count("UnregisterEventHotKey") == 2  # both chords released on stop
    assert fake.names().count("RemoveEventHandler") == 1
    assert {thread for _, thread in fake.calls} == {threading.main_thread()}
    assert not any("EventLoop" in name for name in fake.names())


def test_carbon_install_failure_is_a_registration_error():
    api = carbon_with(FakeCarbon(install_status=-9868))
    with pytest.raises(HotkeyRegistrationError, match="InstallEventHandler failed"):
        api.install()


def test_nothing_in_the_mac_package_runs_a_carbon_event_loop():
    """The crash this guards: RunApplicationEventLoop off the main thread (SIGTRAP on macOS 27)."""
    import pathlib

    import wordink.desktop.mac as mac

    for path in pathlib.Path(mac.__file__).parent.glob("*.py"):
        text = path.read_text(encoding="utf-8")
        assert ".RunApplicationEventLoop(" not in text, path.name
        assert ".QuitApplicationEventLoop(" not in text, path.name
