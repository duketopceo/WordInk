"""The macOS hotkey API: a protocol for tests and a Carbon implementation (plan M3).

`RegisterEventHotKey` (Carbon) is the one hotkey API that needs no Accessibility or Input
Monitoring permission and reports both press and release. `HotkeyApi` is the seam; `CarbonApi`
implements it with `ctypes` and loads the framework only when first used, so this module imports
on every OS. `CarbonApi` has never run on a Mac (plan M9 is the human check), so the pure logic
lives in `hotkey.HotkeySource` and is tested against a fake `HotkeyApi`.
"""

from __future__ import annotations

import ctypes
import ctypes.util
import logging
import sys
import threading
from collections.abc import Callable
from typing import TYPE_CHECKING, Protocol

if TYPE_CHECKING:
    from .hotkey import Chord

log = logging.getLogger("wordink.dictd")

#: called on the API's thread: (hotkey id given to `register`, True for press / False for release)
KeyHandler = Callable[[int, bool], None]


class HotkeyRegistrationError(OSError):
    """The OS refused a chord. `clash` is True when another app or macOS already owns it."""

    def __init__(self, message: str, *, clash: bool = False) -> None:
        super().__init__(message)
        self.clash = clash


class HotkeyApi(Protocol):
    def set_handler(self, handler: KeyHandler) -> None: ...

    def run(self, ready: Callable[[], None]) -> None:
        """Run the event loop on this thread: call `ready()` once registrations can be made, then
        block until `stop()`."""
        ...

    def register(self, hotkey_id: int, chord: Chord) -> None:
        """Register one chord. Raises `HotkeyRegistrationError`."""
        ...

    def unregister_all(self) -> None: ...

    def stop(self) -> None:
        """Make `run` return. Safe to call when it is not running."""
        ...


# Carbon constants.
_K_EVENT_CLASS_KEYBOARD = 0x6B657962  # 'keyb'
_K_EVENT_HOT_KEY_PRESSED, _K_EVENT_HOT_KEY_RELEASED = 5, 6
_K_EVENT_PARAM_DIRECT_OBJECT = 0x2D2D2D2D  # '----'
_TYPE_EVENT_HOT_KEY_ID = 0x686B6964  # 'hkid'
_SIGNATURE = 0x57444B31  # 'WDK1'
_MOD_MASK = {"Cmd": 0x100, "Shift": 0x200, "Option": 0x800, "Ctrl": 0x1000}
_EVENT_HOT_KEY_EXISTS_ERR = -9878


class _EventTypeSpec(ctypes.Structure):
    _fields_ = [("eventClass", ctypes.c_uint32), ("eventKind", ctypes.c_uint32)]


class _EventHotKeyID(ctypes.Structure):
    _fields_ = [("signature", ctypes.c_uint32), ("id", ctypes.c_uint32)]


_HANDLER_TYPE = ctypes.CFUNCTYPE(ctypes.c_int32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)


def carbon_modifiers(chord: Chord) -> int:
    mask = 0
    for m in chord.modifiers:
        mask |= _MOD_MASK[m]
    return mask


class CarbonApi:
    """`RegisterEventHotKey` over ctypes. Only usable on macOS."""

    def __init__(self) -> None:
        self._lib: ctypes.CDLL | None = None
        self._handler: KeyHandler | None = None
        self._refs: dict[int, ctypes.c_void_p] = {}
        self._callback = _HANDLER_TYPE(self._on_event)  # keep a reference so it is never collected
        self._installed = False
        self._lock = threading.Lock()

    def _carbon(self) -> ctypes.CDLL:
        if self._lib is None:
            if sys.platform != "darwin":
                raise HotkeyRegistrationError("Carbon hotkeys exist only on macOS")
            lib = ctypes.CDLL(ctypes.util.find_library("Carbon"))
            lib.GetApplicationEventTarget.restype = ctypes.c_void_p
            lib.InstallEventHandler.argtypes = [
                ctypes.c_void_p, _HANDLER_TYPE, ctypes.c_uint32, ctypes.POINTER(_EventTypeSpec),
                ctypes.c_void_p, ctypes.c_void_p,
            ]  # fmt: skip
            lib.InstallEventHandler.restype = ctypes.c_int32
            lib.GetEventKind.argtypes = [ctypes.c_void_p]
            lib.GetEventKind.restype = ctypes.c_uint32
            lib.GetEventParameter.argtypes = [
                ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p,
                ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p,
            ]  # fmt: skip
            lib.GetEventParameter.restype = ctypes.c_int32
            lib.RegisterEventHotKey.argtypes = [
                ctypes.c_uint32, ctypes.c_uint32, _EventHotKeyID, ctypes.c_void_p, ctypes.c_uint32,
                ctypes.POINTER(ctypes.c_void_p),
            ]  # fmt: skip
            lib.RegisterEventHotKey.restype = ctypes.c_int32
            lib.UnregisterEventHotKey.argtypes = [ctypes.c_void_p]
            lib.UnregisterEventHotKey.restype = ctypes.c_int32
            self._lib = lib
        return self._lib

    def set_handler(self, handler: KeyHandler) -> None:
        self._handler = handler

    def _on_event(self, _call_ref: int, event: int, _user: int) -> int:
        try:
            lib = self._carbon()
            hk = _EventHotKeyID()
            status = lib.GetEventParameter(
                event, _K_EVENT_PARAM_DIRECT_OBJECT, _TYPE_EVENT_HOT_KEY_ID, None,
                ctypes.sizeof(hk), None, ctypes.byref(hk),
            )  # fmt: skip
            if status == 0 and hk.signature == _SIGNATURE and self._handler is not None:
                self._handler(hk.id, lib.GetEventKind(event) == _K_EVENT_HOT_KEY_PRESSED)
        except Exception:  # an exception must never unwind into Carbon
            log.exception("hotkey event handler failed")
        return 0

    def run(self, ready: Callable[[], None]) -> None:
        lib = self._carbon()
        specs = (_EventTypeSpec * 2)(
            _EventTypeSpec(_K_EVENT_CLASS_KEYBOARD, _K_EVENT_HOT_KEY_PRESSED),
            _EventTypeSpec(_K_EVENT_CLASS_KEYBOARD, _K_EVENT_HOT_KEY_RELEASED),
        )
        status = lib.InstallEventHandler(lib.GetApplicationEventTarget(), self._callback, 2, specs, None, None)
        if status != 0:
            raise HotkeyRegistrationError(f"InstallEventHandler failed ({status})")
        self._installed = True
        ready()
        lib.RunApplicationEventLoop()

    def register(self, hotkey_id: int, chord: Chord) -> None:
        lib = self._carbon()
        ref = ctypes.c_void_p()
        status = lib.RegisterEventHotKey(
            chord.key, carbon_modifiers(chord), _EventHotKeyID(_SIGNATURE, hotkey_id),
            lib.GetApplicationEventTarget(), 0, ctypes.byref(ref),
        )  # fmt: skip
        if status != 0:
            raise HotkeyRegistrationError(
                f"RegisterEventHotKey failed ({status})", clash=status == _EVENT_HOT_KEY_EXISTS_ERR
            )
        with self._lock:
            self._refs[hotkey_id] = ref

    def unregister_all(self) -> None:
        with self._lock:
            refs, self._refs = list(self._refs.values()), {}
        if self._lib is not None:
            for ref in refs:
                self._lib.UnregisterEventHotKey(ref)

    def stop(self) -> None:
        if self._lib is not None and self._installed:
            self._lib.QuitApplicationEventLoop()


# -- keyboard events, secure input and Accessibility (plan M4) ----------------------------------------
# Functions here import Quartz or call into system frameworks only when invoked, so the module
# still loads everywhere. None of it has run on a Mac yet (plan M9).

KVK_ANSI_V = 0x09
_FLAG_MASKS = {"Shift": 0x20000, "Ctrl": 0x40000, "Option": 0x80000, "Cmd": 0x100000}


def _framework(name: str) -> ctypes.CDLL:
    if sys.platform != "darwin":
        raise OSError(f"{name} exists only on macOS")
    return ctypes.CDLL(ctypes.util.find_library(name))


def secure_input_enabled() -> bool:
    lib = _framework("Carbon")
    lib.IsSecureEventInputEnabled.restype = ctypes.c_ubyte
    return bool(lib.IsSecureEventInputEnabled())


def accessibility_trusted() -> bool:
    lib = _framework("ApplicationServices")
    lib.AXIsProcessTrusted.restype = ctypes.c_ubyte
    return bool(lib.AXIsProcessTrusted())


def units_to_text(units: list[int]) -> str:
    return b"".join(u.to_bytes(2, "little") for u in units).decode("utf-16-le", "surrogatepass")


def split_batch(events: list[tuple[str, int]]) -> list[tuple[str, object]]:
    """Group consecutive UTF-16 units into one string; key codes stay single. Pure."""
    out: list[tuple[str, object]] = []
    run: list[int] = []
    for kind, value in events:
        if kind == "unit":
            run.append(value)
            continue
        if run:
            out.append(("text", units_to_text(run)))
            run = []
        out.append(("vk", value))
    if run:
        out.append(("text", units_to_text(run)))
    return out


def _post(keycode: int, flags: int, text: str | None = None) -> None:
    import Quartz  # noqa: PLC0415

    for down in (True, False):
        ev = Quartz.CGEventCreateKeyboardEvent(None, keycode, down)
        Quartz.CGEventSetFlags(ev, flags)  # 0 clears Ctrl/Option still held from the hotkey
        if text is not None:
            Quartz.CGEventKeyboardSetUnicodeString(ev, len(text.encode("utf-16-le")) // 2, text)
        Quartz.CGEventPost(Quartz.kCGHIDEventTap, ev)


def post_events(events: list[tuple[str, int]]) -> None:
    for kind, value in split_batch(list(events)):
        if kind == "text":
            _post(0, 0, str(value))
        else:
            _post(int(value), 0)  # type: ignore[arg-type]


def post_paste() -> None:
    _post(KVK_ANSI_V, _FLAG_MASKS["Cmd"])


def wait_modifiers_up(wait: float) -> None:
    """Block up to `wait` seconds until no modifier key is physically held."""
    import time  # noqa: PLC0415

    import Quartz  # noqa: PLC0415

    mask = sum(_FLAG_MASKS.values())
    deadline = time.monotonic() + wait
    while time.monotonic() < deadline:
        if not Quartz.CGEventSourceFlagsState(Quartz.kCGEventSourceStateCombinedSessionState) & mask:
            return
        time.sleep(0.01)
