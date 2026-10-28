"""Tray state mapping, menu wiring and toasts against a fake pystray (plan W6)."""

from __future__ import annotations

import types

import pytest

from wordink.client import WordInkError
from wordink.desktop.interfaces import Indicator
from wordink.desktop.win.tray import ICON_SIZE, MAX_TOOLTIP, STATES, Tray, TrayActions, render_icon, tooltip


class FakeMenuItem:
    def __init__(self, text, action=None) -> None:
        self.text, self.action = text, action


class FakeIcon:
    def __init__(self, name, image, title, menu) -> None:
        self.name, self.icon, self.title, self.menu = name, image, title, menu
        self.running = False
        self.toasts: list[tuple[str, str]] = []

    def run_detached(self):
        self.running = True

    def stop(self):
        self.running = False

    def notify(self, message, title=None):
        self.toasts.append((message, title))


def fake_pystray():
    ps = types.SimpleNamespace(MenuItem=FakeMenuItem, Icon=FakeIcon)

    class Menu:
        SEPARATOR = "---"

        def __init__(self, *items) -> None:
            self.items = items

    ps.Menu = Menu
    return ps


def build():
    calls: list[str] = []
    actions = TrayActions(
        toggle=lambda: calls.append("toggle"),
        copy_last=lambda: calls.append("copy"),
        open_config=lambda: calls.append("config"),
        open_log=lambda: calls.append("log"),
        quit=lambda: calls.append("quit"),
    )
    tray = Tray(actions, pystray=fake_pystray(), make_image=lambda rgba, size: ("img", rgba, size))
    tray.start()
    return tray, tray._icon, calls


def test_it_is_an_indicator_and_every_state_has_a_distinct_non_empty_icon():
    tray, icon, _ = build()
    assert isinstance(tray, Indicator) and icon.running
    images = {s: render_icon(s) for s in STATES}
    assert all(len(b) == ICON_SIZE * ICON_SIZE * 4 and any(b) for b in images.values())
    assert len(set(images.values())) == len(STATES)


def test_states_update_icon_and_tooltip():
    tray, icon, _ = build()
    for state in ("recording", "transcribing", "idle"):
        tray.show(state)
        assert icon.icon[1] == render_icon(state) and icon.title == STATES[state][1]


def test_unchanged_state_does_not_rebuild_the_image():
    tray, icon, _ = build()
    tray.show("recording")
    first = icon.icon
    tray.show("recording")
    assert icon.icon is first


def test_error_shows_detail_and_toasts_once_per_failure():
    tray, icon, _ = build()
    tray.show("error", "no microphone found")
    tray.show("error", "no microphone found")
    assert icon.toasts == [("no microphone found", "WordInk")]
    assert icon.title.endswith("no microphone found")
    tray.show("idle")
    tray.show("error", "second failure")
    assert [t[0] for t in icon.toasts] == ["no microphone found", "second failure"]


def test_a_long_error_is_truncated_for_the_tooltip():
    assert len(tooltip("error", "x" * 500)) == MAX_TOOLTIP
    assert tooltip("recording", "ignored") == STATES["recording"][1]


def test_menu_items_call_the_right_actions():
    tray, icon, calls = build()
    items = [i for i in icon.menu.items if isinstance(i, FakeMenuItem)]
    assert [i.text for i in items] == [
        "Start / stop dictation", "Copy last transcript", "Open config folder", "Open log", "Quit WordInk",
    ]
    for i in items[:4]:
        i.action(None, None)
    assert calls == ["toggle", "copy", "config", "log"]


def test_quit_stops_the_daemon_then_the_icon():
    tray, icon, calls = build()
    next(i for i in icon.menu.items if getattr(i, "text", "") == "Quit WordInk").action(None, None)
    assert calls == ["quit"] and not icon.running and tray._icon is None
    tray.show("idle")  # after quit, updates are ignored quietly


def test_show_before_start_is_remembered_and_unknown_states_map_to_off():
    actions = TrayActions(*(lambda: None,) * 5)
    tray = Tray(actions, pystray=fake_pystray(), make_image=lambda *a: None)
    tray.show("recording")
    assert tray.state == "recording"
    tray.show("weird")
    assert tray.state == "off"


def test_a_tray_glitch_never_raises():
    tray, icon, _ = build()

    def boom(*a):
        raise RuntimeError("shell gone")

    icon.notify = boom
    tray.show("error", "x")  # must not raise


def test_missing_pystray_says_how_to_fix_it(monkeypatch):
    import sys

    monkeypatch.setitem(sys.modules, "pystray", None)
    with pytest.raises(WordInkError, match="windows' extra"):
        Tray(TrayActions(*(lambda: None,) * 5)).start()
