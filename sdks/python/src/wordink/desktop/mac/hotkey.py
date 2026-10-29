"""Hotkey chords for macOS: parse `Ctrl+Option+D` into key codes and refuse reserved ones.

The parser (plan M1) is pure. `HotkeySource` (plan M3) registers the two chords through the
`HotkeyApi` in `quartz.py` and turns press and release events into daemon callbacks. The key
codes are the Carbon `kVK_*` virtual key codes, which name physical keys on an ANSI layout.
"""

from __future__ import annotations

import asyncio
import logging
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass

from ...client import WordInkError
from .quartz import HotkeyApi, HotkeyRegistrationError

log = logging.getLogger("wordink.dictd")

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


# -- the HotkeySource (plan M3) ----------------------------------------------------------------------

HOLD_ID, TOGGLE_ID = 1, 2


class HotkeyClash(WordInkError):
    """A chord could not be registered, usually because macOS or another app already owns it."""


class HotkeySource:
    """Press/release for the hold chord and press for the toggle chord, over a `HotkeyApi`.

    * The API's event loop runs on its own thread; `start()` waits for it and registers both
      chords, raising `HotkeyClash` (and registering nothing) if either is refused.
    * Callbacks go through `loop.call_soon_threadsafe` when a loop is given, else run on the API
      thread. A callback that raises is logged and never kills the event loop.
    * Key repeat and a stray release are ignored, so the daemon sees strict press/release pairs.
    * After sleep and wake the registrations are redone (the wake is detected by the wall clock
      jumping ahead of the monotonic clock, which stops while the Mac sleeps) and a hold that was
      in progress is released, because its key-up was probably lost.
    """

    def __init__(
        self,
        hold: Chord,
        toggle: Chord,
        api: HotkeyApi,
        *,
        loop: asyncio.AbstractEventLoop | None = None,
        start_timeout: float = 3.0,
        wake_poll_interval: float = 5.0,
        wake_gap: float = 10.0,
        wall: Callable[[], float] = time.time,
        mono: Callable[[], float] = time.monotonic,
    ) -> None:
        self.hold, self.toggle, self.api, self.loop = hold, toggle, api, loop
        self.start_timeout, self.wake_poll_interval, self.wake_gap = start_timeout, wake_poll_interval, wake_gap
        self._wall, self._mono = wall, mono
        self._offset = wall() - mono()
        self.reregistrations = 0
        self._handlers: dict[str, Callable[[], None]] = {}
        self._holding = False
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._threads: list[threading.Thread] = []
        self._run_error: Exception | None = None

    # -- events (API thread) ------------------------------------------------------------------------
    def _call(self, name: str) -> None:
        handler = self._handlers.get(name)
        if handler is None:
            return
        if self.loop is not None:
            self.loop.call_soon_threadsafe(self._safe, handler)
        else:
            self._safe(handler)

    @staticmethod
    def _safe(handler: Callable[[], None]) -> None:
        try:
            handler()
        except Exception:
            log.exception("hotkey handler failed")

    def _on_key(self, hotkey_id: int, pressed: bool) -> None:
        if hotkey_id == HOLD_ID:
            with self._lock:
                if pressed == self._holding:  # key repeat, or a release with no press
                    return
                self._holding = pressed
            self._call("hold_press" if pressed else "hold_release")
        elif hotkey_id == TOGGLE_ID and pressed:
            self._call("toggle")

    # -- registration ---------------------------------------------------------------------------------
    def _register_all(self) -> None:
        for hotkey_id, chord in ((HOLD_ID, self.hold), (TOGGLE_ID, self.toggle)):
            try:
                self.api.register(hotkey_id, chord)
            except HotkeyRegistrationError as exc:
                self.api.unregister_all()
                why = "is already used by macOS or another app" if exc.clash else "could not be registered"
                raise HotkeyClash(f"hotkey {chord.text} {why}; pick another in the config") from exc

    def _run(self, ready: threading.Event) -> None:
        try:
            self.api.run(ready.set)
        except Exception as exc:
            self._run_error = exc
            log.error("hotkey event loop stopped: %s", type(exc).__name__)
        finally:
            ready.set()  # never leave start() waiting

    # -- wake detection -----------------------------------------------------------------------------
    def check_wake(self) -> bool:
        """True (after re-registering) when the Mac slept since the last check."""
        offset = self._wall() - self._mono()
        slept = offset - self._offset
        self._offset = offset
        if slept < self.wake_gap:
            return False
        log.info("woke from sleep; re-registering hotkeys")
        with self._lock:
            was_holding, self._holding = self._holding, False
        self.api.unregister_all()
        try:
            self._register_all()
            self.reregistrations += 1
        except HotkeyClash as exc:
            log.error("%s", exc)
        if was_holding:
            self._call("hold_release")  # never leave the microphone recording
        return True

    def _watch_wake(self) -> None:
        while not self._stop.wait(self.wake_poll_interval):
            try:
                self.check_wake()
            except Exception as exc:
                log.error("wake check: %s", type(exc).__name__)

    # -- HotkeySource -----------------------------------------------------------------------------------
    def start(
        self,
        on_hold_press: Callable[[], None],
        on_hold_release: Callable[[], None],
        on_toggle: Callable[[], None],
    ) -> None:
        if self._threads:
            raise WordInkError("the hotkey source is already running")
        self._handlers = {"hold_press": on_hold_press, "hold_release": on_hold_release, "toggle": on_toggle}
        self._stop.clear()
        self._holding = False
        self._run_error = None
        self._offset = self._wall() - self._mono()
        self.api.set_handler(self._on_key)
        ready = threading.Event()
        loop_thread = threading.Thread(target=self._run, args=(ready,), name="wordink-hotkeys", daemon=True)
        self._threads.append(loop_thread)
        loop_thread.start()
        try:
            if not ready.wait(self.start_timeout) or self._run_error is not None:
                reason = type(self._run_error).__name__ if self._run_error else "timed out"
                raise HotkeyClash(f"cannot start the hotkey event loop ({reason})")
            self._register_all()
        except Exception:
            self.stop()
            raise
        watcher = threading.Thread(target=self._watch_wake, name="wordink-wake", daemon=True)
        self._threads.append(watcher)
        watcher.start()

    def stop(self) -> None:
        self._stop.set()
        self.api.unregister_all()
        self.api.stop()
        for t in self._threads:
            t.join(2.0)
        self._threads = []
        self._holding = False
