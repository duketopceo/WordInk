"""macOS menu bar against a fake `rumps` (plan M5)."""

from __future__ import annotations

import threading
from types import SimpleNamespace

import pytest

from wordink.client import WordInkError
from wordink.desktop.interfaces import Indicator
from wordink.desktop.mac.menubar import STATES, MAX_TOOLTIP, MenuActions, MenuBar, tooltip

MAIN = threading.current_thread()


def on_main(what: str) -> None:
    assert threading.current_thread() is MAIN, f"{what} touched off the main thread"


class FakeItem:
    def __init__(self, title, callback=None):
        self._title, self.callback = title, callback

    @property
    def title(self):
        return self._title

    @title.setter
    def title(self, v):
        on_main("menu item")
        self._title = v

    def set_callback(self, cb):
        self.callback = cb


class FakeApp:
    last = None

    def __init__(self, name, title=None, menu=None, quit_button=None):
        self.name, self._title, self.menu, self.ran = name, title, menu, False
        FakeApp.last = self

    @property
    def title(self):
        return self._title

    @title.setter
    def title(self, v):
        on_main("status item")
        self._title = v

    def run(self):
        self.ran = True


class FakeTimer:
    last = None

    def __init__(self, cb, interval):
        self.cb, self.interval, self.started = cb, interval, False
        FakeTimer.last = self

    def start(self):
        self.started = True


def fake_rumps():
    r = SimpleNamespace(MenuItem=FakeItem, App=FakeApp, Timer=FakeTimer, notes=[], quit_calls=0)
    r.notification = lambda title, sub, msg: (on_main("notification"), r.notes.append(msg))
    r.quit_application = lambda: setattr(r, "quit_calls", r.quit_calls + 1)
    return r


def started(actions=None):
    r = fake_rumps()
    bar = MenuBar(actions, rumps=r)
    bar.run()
    return bar, r, FakeApp.last


def test_it_is_an_indicator_and_run_builds_the_status_item_and_a_timer():
    bar, r, app = started()
    assert isinstance(bar, Indicator) and app.ran and app.title == STATES["off"][0]
    labels = [i.title for i in app.menu if i is not None]
    assert labels == [
        "WordInk: stopped", "Start / stop dictation", "Copy last transcript", "Open config folder", "Open log", "Quit WordInk",
    ]
    assert app.menu.count(None) == 3


def test_every_state_maps_to_a_distinct_symbol_and_tooltip():
    assert len({v[0] for v in STATES.values()}) == len(STATES)
    bar, _, app = started()
    status = app.menu[0]
    for state in ("idle", "recording", "transcribing", "error", "off"):
        bar.show(state)
        bar.drain()
        assert (app.title, status.title.split(":")[0]) == (STATES[state][0], "WordInk")
        assert status.title == STATES[state][1] or state == "error"
    bar.show("nonsense")
    bar.drain()
    assert app.title == STATES["off"][0]


def test_an_error_detail_goes_in_the_tooltip_and_long_ones_are_cut():
    assert tooltip("error", "no mic") == "WordInk: error: no mic"
    assert tooltip("idle", "ignored") == "WordInk: ready"
    assert len(tooltip("error", "x" * 500)) == MAX_TOOLTIP


def test_an_error_notifies_once_per_distinct_failure_until_the_state_recovers():
    bar, r, _ = started()
    for _ in range(3):
        bar.show("error", "Groq 401")
    bar.drain()
    assert r.notes == ["Groq 401"]
    bar.show("error", "network down")
    bar.drain()
    assert r.notes == ["Groq 401", "network down"]
    bar.show("idle")
    bar.show("error", "network down")
    bar.drain()
    assert r.notes[-1] == "network down" and len(r.notes) == 3


def test_notice_posts_a_notification_without_changing_state():
    bar, r, app = started()
    bar.show("idle")
    bar.notice("Running in toggle-only mode")
    bar.drain()
    assert r.notes == ["Running in toggle-only mode"] and bar.state == "idle"


def test_menu_actions_call_the_right_handlers():
    calls = []
    acts = MenuActions(*(lambda n=n: calls.append(n) for n in ("toggle", "copy", "config", "log", "quit")))
    bar, r, app = started(acts)
    by_title = {i.title: i for i in app.menu if i is not None}
    for t in ("Start / stop dictation", "Copy last transcript", "Open config folder", "Open log", "Quit WordInk"):
        by_title[t].callback(by_title[t])
    assert calls == ["toggle", "copy", "config", "log", "quit"] and r.quit_calls == 1


def test_quit_still_closes_the_menu_if_the_daemon_shutdown_raises():
    def boom():
        raise RuntimeError("daemon")

    bar, r, app = started(MenuActions(lambda: 0, lambda: 0, lambda: 0, lambda: 0, boom))
    quit_item = next(i for i in app.menu if i is not None and i.title == "Quit WordInk")
    with pytest.raises(RuntimeError):
        quit_item.callback(quit_item)
    assert r.quit_calls == 1


def test_the_ui_is_only_touched_on_the_main_thread_even_when_the_daemon_posts_from_another():
    bar, r, app = started()

    def daemon():
        bar.show("recording")
        bar.show("error", "boom")
        bar.notice("hi")

    t = threading.Thread(target=daemon)
    t.start()
    t.join()
    assert app.title == STATES["off"][0] and r.notes == []  # nothing applied from the worker thread
    bar.drain()  # the main-thread timer
    assert app.title == STATES["error"][0] and r.notes == ["boom", "hi"]


def test_a_failing_ui_update_is_logged_and_does_not_stop_later_updates(caplog):
    bar, r, app = started()
    r.notification = lambda *a: (_ for _ in ()).throw(RuntimeError("center down"))
    bar.show("error", "x")
    bar.show("idle")
    bar.drain()
    assert bar.state == "idle" and "menu bar update failed" in caplog.text


def test_show_before_run_is_queued_and_applied_after():
    r = fake_rumps()
    bar = MenuBar(rumps=r)
    bar.show("recording")
    bar.run()
    bar.drain()
    assert FakeApp.last.title == STATES["recording"][0]


def test_the_timer_is_started_and_drains_the_queue():
    bar, r, app = started()
    timer = FakeTimer.last
    assert timer.started and timer.interval == 0.2
    bar.show("transcribing")
    timer.cb(timer)
    assert app.title == STATES["transcribing"][0]


def test_without_rumps_run_explains_the_extra(monkeypatch):
    import builtins

    real = builtins.__import__

    def deny(name, *a, **k):
        if name == "rumps":
            raise ImportError("no rumps")
        return real(name, *a, **k)

    monkeypatch.setattr(builtins, "__import__", deny)
    with pytest.raises(WordInkError, match="macos"):
        MenuBar().run()
