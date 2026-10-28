"""Insertion against real fake executables on a private PATH: each logs its argv and stdin,
and wl-copy / wl-paste share a clipboard file, so the paste-and-restore dance is observable."""

from __future__ import annotations

import json
import stat
import textwrap
from pathlib import Path

import pytest

from wordink.client import WordInkError
from wordink.desktop.inject import InjectConfig, Injector

SCRIPTS = {
    "wtype": 'echo "wtype $*" >> "$FAKE/log"; [ -f "$FAKE/wtype_fail" ] && exit 1; exit 0',
    "ydotool": 'echo "ydotool $*" >> "$FAKE/log"; [ -f "$FAKE/ydotool_fail" ] && exit 1; exit 0',
    "wl-copy": 'if [ "$1" = "--clear" ]; then echo "wl-copy --clear" >> "$FAKE/log"; : > "$FAKE/clipboard"; exit 0; fi\n'
    'cat > "$FAKE/clipboard"; echo "wl-copy <$(cat "$FAKE/clipboard")>" >> "$FAKE/log"',
    "wl-paste": '[ -s "$FAKE/clipboard" ] || exit 1; cat "$FAKE/clipboard"',
    "notify-send": 'echo "notify $*" >> "$FAKE/log"',
    "hyprctl": 'cat "$FAKE/window.json" 2>/dev/null || exit 1',
}


class Rig:
    def __init__(self, root: Path, tools: list[str]) -> None:
        self.root = root
        self.bin = root / "bin"
        self.bin.mkdir()
        for name in tools:
            exe = self.bin / name
            exe.write_text(f"#!/bin/sh\n{textwrap.dedent(SCRIPTS[name])}\n")
            exe.chmod(exe.stat().st_mode | stat.S_IXUSR)
        (root / "log").write_text("")
        (root / "clipboard").write_text("")

    def window(self, cls: str) -> None:
        (self.root / "window.json").write_text(json.dumps({"class": cls, "title": "x"}))

    def fail(self, tool: str) -> None:
        (self.root / f"{tool}_fail").write_text("")

    def clipboard(self, text: str) -> None:
        (self.root / "clipboard").write_text(text)

    @property
    def log(self) -> list[str]:
        return (self.root / "log").read_text().splitlines()

    def injector(self, **cfg) -> Injector:
        return Injector(InjectConfig(**cfg), path=str(self.bin), sleep=lambda s: None)


ALL = list(SCRIPTS)


@pytest.fixture
def rig(tmp_path, monkeypatch):
    monkeypatch.setenv("FAKE", str(tmp_path))
    return lambda tools=ALL: Rig(tmp_path, tools)


def test_types_with_wtype_into_an_ordinary_window(rig):
    r = rig()
    r.window("firefox")
    res = r.injector().insert("hello — wörld")
    assert (res.method, res.detail) == ("typed", "wtype")
    assert r.log == ["wtype -- hello — wörld"]


def test_text_starting_with_a_dash_is_not_taken_for_an_option(rig):
    r = rig()
    r.window("firefox")
    r.injector().insert("-M ctrl")
    assert r.log == ["wtype -- -M ctrl"]


def test_falls_back_to_ydotool_when_wtype_fails(rig):
    r = rig()
    r.window("firefox")
    r.fail("wtype")
    res = r.injector().insert("plain ascii")
    assert (res.method, res.detail) == ("typed", "ydotool")
    assert r.log[-1] == "ydotool type --key-delay 2 -- plain ascii"


def test_falls_back_to_ydotool_when_wtype_is_not_installed(rig):
    r = rig([t for t in ALL if t != "wtype"])
    r.window("firefox")
    assert r.injector().insert("ascii only").detail == "ydotool"


def test_ydotool_is_never_given_non_ascii_text_it_would_mangle(rig):
    r = rig([t for t in ALL if t != "wtype"])
    r.window("firefox")
    res = r.injector().insert("café")
    assert res.method == "clipboard"
    assert not any(line.startswith("ydotool type") for line in r.log)
    assert (r.root / "clipboard").read_text() == "café"


def test_clipboard_fallback_when_nothing_can_type_and_the_user_is_told(rig):
    r = rig()
    r.window("firefox")
    r.fail("wtype")
    r.fail("ydotool")
    res = r.injector().insert("hello there")
    assert res.method == "clipboard"
    assert (r.root / "clipboard").read_text() == "hello there"
    assert any(line.startswith("notify") and "clipboard" in line for line in r.log)


def test_terminal_gets_a_paste_and_the_old_clipboard_comes_back(rig):
    r = rig()
    r.window("Alacritty")
    r.clipboard("my precious clipboard")
    res = r.injector().insert("ls -la")
    assert (res.method, res.detail) == ("pasted", "alacritty")
    assert r.log == [
        "wl-copy <ls -la>",
        "wtype -M ctrl -M shift -k v -m shift -m ctrl",
        "wl-copy <my precious clipboard>",
    ]
    assert (r.root / "clipboard").read_text() == "my precious clipboard"
    assert not any(line.startswith("wtype --") for line in r.log), "terminals must not be typed key by key"


def test_terminal_paste_clears_the_clipboard_if_it_was_empty(rig):
    r = rig()
    r.window("kitty")
    r.injector().insert("echo hi")
    assert r.log[-1] == "wl-copy --clear"
    assert (r.root / "clipboard").read_text() == ""


def test_terminal_paste_can_use_ydotool_for_the_chord(rig):
    r = rig()
    r.window("foot")
    r.fail("wtype")
    r.injector().insert("ls")
    assert "ydotool key 29:1 42:1 47:1 47:0 42:0 29:0" in r.log


def test_terminal_where_the_chord_cannot_be_sent_leaves_text_on_the_clipboard(rig):
    r = rig()
    r.window("kitty")
    r.fail("wtype")
    r.fail("ydotool")
    res = r.injector().insert("ls")
    assert res.method == "clipboard"
    assert (r.root / "clipboard").read_text() == "ls"


def test_password_managers_are_never_typed_into(rig):
    r = rig()
    r.window("KeePassXC")
    res = r.injector().insert("hunter2")
    assert (res.method, res.detail) == ("skipped", "denied window")
    assert not any(line.startswith(("wtype", "ydotool", "wl-copy")) for line in r.log)
    assert any(line.startswith("notify") for line in r.log)


def test_unknown_focus_still_types(rig):
    r = rig([t for t in ALL if t != "hyprctl"])
    assert r.injector().insert("no compositor info").method == "typed"


def test_a_failing_hyprctl_still_types(rig):
    r = rig()  # hyprctl present but exits non-zero (no window.json)
    assert r.injector().insert("still typed").method == "typed"


def test_custom_order_and_terminal_list(rig):
    r = rig()
    r.window("myterm")
    r.injector(order=("ydotool",), terminals=frozenset({"myterm"})).insert("x")
    assert r.log[0] == "wl-copy <x>" and r.log[1].startswith("ydotool key")


def test_nothing_installed_is_an_actionable_error(rig):
    r = rig(["hyprctl"])
    r.window("firefox")
    with pytest.raises(WordInkError, match="install wtype"):
        r.injector().insert("hello")


def test_empty_text_is_a_no_op(rig):
    r = rig()
    assert r.injector().insert("").method == "skipped"
    assert r.log == []


def test_notifications_can_be_turned_off(rig):
    r = rig()
    r.window("firefox")
    r.fail("wtype")
    r.fail("ydotool")
    r.injector(notify=False).insert("hello")
    assert not any(line.startswith("notify") for line in r.log)
