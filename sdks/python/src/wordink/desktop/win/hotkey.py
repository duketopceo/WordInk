"""Hotkey chords for Windows: parse `Ctrl+Alt+D` into virtual-key codes and refuse reserved ones.

This module is the pure half of the hotkey layer (W2). The low-level hook that watches the keys
lives in the same file later (W4) and goes through `win32.py`; nothing here calls an OS API.
"""

from __future__ import annotations

from dataclasses import dataclass

from ...client import WordInkError

DEFAULT_HOLD = "Ctrl+Alt+D"
DEFAULT_TOGGLE = "Ctrl+Alt+S"

VK_SHIFT, VK_CONTROL, VK_MENU = 0x10, 0x11, 0x12  # Menu is Alt

#: modifier name -> canonical name. `Win` is deliberately absent: Windows owns those combinations.
_MODIFIERS = {
    "CTRL": "Ctrl",
    "CONTROL": "Ctrl",
    "ALT": "Alt",
    "SHIFT": "Shift",
}
_MODIFIER_VK = {"Ctrl": VK_CONTROL, "Alt": VK_MENU, "Shift": VK_SHIFT}
_WIN_NAMES = {"WIN", "WINDOWS", "SUPER", "META", "CMD", "LWIN", "RWIN"}

_SHOWN = {"PAGEUP": "PageUp", "PAGEDOWN": "PageDown", "ESC": "Esc", "ESCAPE": "Esc"}

_NAMED_KEYS = {
    "SPACE": 0x20,
    "PAUSE": 0x13,
    "TAB": 0x09,
    "ENTER": 0x0D,
    "ESC": 0x1B,
    "ESCAPE": 0x1B,
    "BACKSPACE": 0x08,
    "INSERT": 0x2D,
    "DELETE": 0x2E,
    "HOME": 0x24,
    "END": 0x23,
    "PAGEUP": 0x21,
    "PAGEDOWN": 0x22,
    "UP": 0x26,
    "DOWN": 0x28,
    "LEFT": 0x25,
    "RIGHT": 0x27,
}

#: chords that are never usable: they close windows, switch tasks or open the secure attention screen.
_RESERVED = {
    ("Alt",): {0x73, 0x09, 0x1B},  # Alt+F4, Alt+Tab, Alt+Esc
    ("Ctrl",): {0x1B, 0x43, 0x56, 0x58, 0x5A},  # Ctrl+Esc, Ctrl+C/V/X/Z
    ("Alt", "Ctrl"): {0x2E},  # Ctrl+Alt+Delete
    ("Ctrl", "Shift"): {0x1B},  # Ctrl+Shift+Esc (Task Manager)
}


@dataclass(frozen=True)
class Chord:
    modifiers: tuple[str, ...]  # canonical, sorted: Alt, Ctrl, Shift
    key: int  # virtual-key code of the main key
    text: str  # normalized spelling, e.g. "Ctrl+Alt+D"

    @property
    def modifier_vks(self) -> tuple[int, ...]:
        return tuple(_MODIFIER_VK[m] for m in self.modifiers)


def _key_code(name: str, chord: str) -> tuple[int, str]:
    up = name.upper()
    if len(up) == 1 and up.isascii() and up.isalnum():
        return ord(up), up  # VK_A..VK_Z and VK_0..VK_9 equal their ASCII codes
    if up in _NAMED_KEYS:
        return _NAMED_KEYS[up], _SHOWN.get(up, name.capitalize())
    if up.startswith("F") and up[1:].isdigit() and 1 <= int(up[1:]) <= 24:
        return 0x6F + int(up[1:]), up  # VK_F1 is 0x70
    raise WordInkError(f"hotkey {chord!r}: unknown key {name!r} (use a letter, digit, F1-F24, Space, Pause, Tab, ...)")


def parse_chord(chord: str) -> Chord:
    """`"Ctrl+Alt+D"` -> `Chord`. At least one modifier is required so normal typing is never captured."""
    parts = [p.strip() for p in chord.split("+")]
    if len(parts) < 2 or any(not p for p in parts):
        raise WordInkError(f"hotkey {chord!r} must look like 'Ctrl+Alt+D' (modifiers, then one key)")
    *mod_names, key_name = parts
    mods: set[str] = set()
    for m in mod_names:
        up = m.upper()
        if up in _WIN_NAMES:
            raise WordInkError(f"hotkey {chord!r}: Windows reserves the Win key; use Ctrl, Alt or Shift")
        if up not in _MODIFIERS:
            raise WordInkError(f"hotkey {chord!r}: {m!r} is not a modifier (use Ctrl, Alt or Shift)")
        mods.add(_MODIFIERS[up])
    if key_name.upper() in _MODIFIERS or key_name.upper() in _WIN_NAMES:
        raise WordInkError(f"hotkey {chord!r} must end with a normal key, not a modifier")
    code, shown = _key_code(key_name, chord)
    ordered = tuple(sorted(mods))
    if code in _RESERVED.get(ordered, ()):
        raise WordInkError(f"hotkey {chord!r} is reserved by Windows; pick another")
    return Chord(ordered, code, "+".join([*ordered, shown]))


def parse_chords(hold: str, toggle: str) -> tuple[Chord, Chord]:
    """Parse both chords; they must differ (a hold equal to the toggle would start and stop at once)."""
    h, t = parse_chord(hold), parse_chord(toggle)
    if (h.modifiers, h.key) == (t.modifiers, t.key):
        raise WordInkError("the hold and toggle hotkeys must be different keys")
    return h, t
