"""Hotkey chords for macOS: parse `Ctrl+Option+D` into key codes and refuse reserved ones.

This is the pure half of the hotkey layer (plan M1). The registration that watches the keys
comes later (M3) and goes through `quartz.py`; nothing here calls an OS API. The key codes are
the Carbon `kVK_*` virtual key codes, which name physical keys on an ANSI layout.
"""

from __future__ import annotations

from dataclasses import dataclass

from ...client import WordInkError

DEFAULT_HOLD = "Ctrl+Option+D"
DEFAULT_TOGGLE = "Ctrl+Option+S"

#: modifier name -> canonical name. `Alt` is the PC name for Option.
_MODIFIERS = {
    "CTRL": "Ctrl",
    "CONTROL": "Ctrl",
    "OPTION": "Option",
    "OPT": "Option",
    "ALT": "Option",
    "SHIFT": "Shift",
    "CMD": "Cmd",
    "COMMAND": "Cmd",
}
_ORDER = ("Cmd", "Ctrl", "Option", "Shift")
_FN_NAMES = {"FN", "FUNCTION", "GLOBE"}
_WIN_NAMES = {"WIN", "WINDOWS", "SUPER", "META"}

# Carbon kVK_ANSI_* and kVK_* codes.
_LETTERS = {
    "A": 0x00, "S": 0x01, "D": 0x02, "F": 0x03, "H": 0x04, "G": 0x05, "Z": 0x06, "X": 0x07, "C": 0x08, "V": 0x09,
    "B": 0x0B, "Q": 0x0C, "W": 0x0D, "E": 0x0E, "R": 0x0F, "Y": 0x10, "T": 0x11, "O": 0x1F, "U": 0x20, "I": 0x22,
    "P": 0x23, "L": 0x25, "J": 0x26, "K": 0x28, "N": 0x2D, "M": 0x2E,
}  # fmt: skip
_DIGITS = {"1": 0x12, "2": 0x13, "3": 0x14, "4": 0x15, "6": 0x16, "5": 0x17, "9": 0x19, "7": 0x1A, "8": 0x1C, "0": 0x1D}
_FKEYS = {
    1: 0x7A, 2: 0x78, 3: 0x63, 4: 0x76, 5: 0x60, 6: 0x61, 7: 0x62, 8: 0x64, 9: 0x65, 10: 0x6D,
    11: 0x67, 12: 0x6F, 13: 0x69, 14: 0x6B, 15: 0x71, 16: 0x6A, 17: 0x40, 18: 0x4F, 19: 0x50, 20: 0x5A,
}  # fmt: skip
# ENTER is the keypad key (kVK_ANSI_KeypadEnter); RETURN is the main one.
_NAMED = {
    "RETURN": (0x24, "Return"), "ENTER": (0x4C, "Enter"), "TAB": (0x30, "Tab"), "SPACE": (0x31, "Space"),
    "BACKSPACE": (0x33, "Delete"), "DELETE": (0x33, "Delete"), "ESC": (0x35, "Esc"), "ESCAPE": (0x35, "Esc"),
    "LEFT": (0x7B, "Left"), "RIGHT": (0x7C, "Right"), "DOWN": (0x7D, "Down"), "UP": (0x7E, "Up"),
}  # fmt: skip

_KEY_NAMES = {code: name for name, code in _LETTERS.items()} | {code: name for name, code in _DIGITS.items()}

SPACE, TAB, ESC = 0x31, 0x30, 0x35

#: (sorted modifiers) -> {key code: what owns it}. Chords macOS or every app uses.
_RESERVED: dict[tuple[str, ...], dict[int, str]] = {
    ("Cmd",): {
        SPACE: "Spotlight", TAB: "the app switcher", _LETTERS["Q"]: "Quit", _LETTERS["H"]: "Hide",
        _LETTERS["M"]: "Minimize", _LETTERS["W"]: "Close window", _LETTERS["C"]: "Copy", _LETTERS["V"]: "Paste",
        _LETTERS["X"]: "Cut", _LETTERS["Z"]: "Undo", _LETTERS["A"]: "Select All",
    },
    ("Cmd", "Ctrl"): {_LETTERS["Q"]: "Lock Screen", SPACE: "the emoji picker"},
    ("Cmd", "Option"): {ESC: "Force Quit", _LETTERS["D"]: "Dock hiding", SPACE: "Finder search"},
    ("Cmd", "Shift"): {_DIGITS["3"]: "screenshots", _DIGITS["4"]: "screenshots", _DIGITS["5"]: "screenshots"},
    ("Ctrl",): {SPACE: "input source switching"},
    ("Cmd", "Ctrl", "Option"): {},
}


@dataclass(frozen=True)
class Chord:
    modifiers: tuple[str, ...]  # canonical, in the order Cmd, Ctrl, Option, Shift
    key: int  # Carbon virtual key code of the main key
    text: str  # normalized spelling, e.g. "Ctrl+Option+D"


def _key_code(name: str, chord: str) -> tuple[int, str]:
    up = name.upper()
    if up in _LETTERS:
        return _LETTERS[up], up
    if up in _DIGITS:
        return _DIGITS[up], up
    if up in _NAMED:
        return _NAMED[up]
    if up.startswith("F") and up[1:].isdigit() and int(up[1:]) in _FKEYS:
        return _FKEYS[int(up[1:])], up
    raise WordInkError(f"hotkey {chord!r}: unknown key {name!r} (use a letter, digit, F1-F20, Space, Tab, Return, ...)")


def parse_chord(chord: str) -> Chord:
    """`"Ctrl+Option+D"` -> `Chord`. At least one modifier is required so normal typing is never captured."""
    parts = [p.strip() for p in chord.split("+")]
    if len(parts) < 2 or any(not p for p in parts):
        raise WordInkError(f"hotkey {chord!r} must look like 'Ctrl+Option+D' (modifiers, then one key)")
    *mod_names, key_name = parts
    mods: set[str] = set()
    for m in mod_names:
        up = m.upper()
        if up in _FN_NAMES:
            raise WordInkError(f"hotkey {chord!r}: the Fn/Globe key cannot be part of a chord; use Ctrl, Option, Shift or Cmd")
        if up in _WIN_NAMES:
            raise WordInkError(f"hotkey {chord!r}: {m!r} is not a Mac key; use Cmd for the Command key")
        if up not in _MODIFIERS:
            raise WordInkError(f"hotkey {chord!r}: {m!r} is not a modifier (use Ctrl, Option, Shift or Cmd)")
        mods.add(_MODIFIERS[up])
    if key_name.upper() in _MODIFIERS or key_name.upper() in _FN_NAMES | _WIN_NAMES:
        raise WordInkError(f"hotkey {chord!r} must end with a normal key, not a modifier")
    code, shown = _key_code(key_name, chord)
    ordered = tuple(m for m in _ORDER if m in mods)
    owner = _RESERVED.get(ordered, {}).get(code)
    if owner:
        raise WordInkError(f"hotkey {chord!r} is reserved by macOS ({owner}); pick another")
    if ordered == ("Shift",):
        raise WordInkError(f"hotkey {chord!r}: Shift alone is typing; add Ctrl, Option or Cmd")
    return Chord(ordered, code, "+".join([*ordered, shown]))


def parse_chords(hold: str, toggle: str) -> tuple[Chord, Chord]:
    """Parse both chords; they must differ (a hold equal to the toggle would start and stop at once)."""
    h, t = parse_chord(hold), parse_chord(toggle)
    if (h.modifiers, h.key) == (t.modifiers, t.key):
        raise WordInkError("the hold and toggle hotkeys must be different keys")
    return h, t
