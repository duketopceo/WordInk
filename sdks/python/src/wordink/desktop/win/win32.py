"""The only module that touches `ctypes.windll` (plan W5b).

Declarations only: importing this module on any OS calls nothing and loads no DLL. Every Windows
call happens inside a method of `Win32`, which fetches the DLLs on first use and raises a clear
error off Windows. `build_inputs` and the structures are pure, so they are tested on Linux.
"""

from __future__ import annotations

import ctypes
import os
import sys
import time
from collections.abc import Sequence
from ctypes import wintypes
from dataclasses import dataclass

from ...client import WordInkError
from .insert import ClipboardBusy, ClipboardSnapshot, Event

# -- constants -------------------------------------------------------------------------------------
INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004

VK_SHIFT, VK_CONTROL, VK_MENU = 0x10, 0x11, 0x12
_MODIFIER_VKS = (VK_SHIFT, VK_CONTROL, VK_MENU)

CF_TEXT, CF_OEMTEXT, CF_UNICODETEXT, CF_LOCALE = 1, 7, 13, 16
_TEXT_FORMATS = {CF_TEXT, CF_OEMTEXT, CF_UNICODETEXT, CF_LOCALE, 1 + 0x80}  # 0x81 is CF_DSPTEXT
GMEM_MOVEABLE = 0x0002
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
TOKEN_QUERY = 0x0008
TOKEN_ELEVATION = 20  # TokenElevation

#: registered clipboard formats that keep text out of Win+V history, the cloud clipboard and monitors
EXCLUDE_FORMATS = (
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
)


# -- structures ------------------------------------------------------------------------------------
class KEYBDINPUT(ctypes.Structure):
    _fields_ = (
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    )


class MOUSEINPUT(ctypes.Structure):  # present only so the INPUT union has its real size
    _fields_ = (
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    )


class _INPUT_UNION(ctypes.Union):
    _fields_ = (("ki", KEYBDINPUT), ("mi", MOUSEINPUT))


class INPUT(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = (("type", wintypes.DWORD), ("u", _INPUT_UNION))


def _key(vk: int = 0, scan: int = 0, flags: int = 0) -> INPUT:
    return INPUT(type=INPUT_KEYBOARD, ki=KEYBDINPUT(wVk=vk, wScan=scan, dwFlags=flags, time=0, dwExtraInfo=0))


def build_inputs(events: Sequence[Event]) -> list[INPUT]:
    """A down and an up per event: Unicode units by scan code, virtual keys by code."""
    out: list[INPUT] = []
    for kind, code in events:
        if kind == "unit":
            out += [_key(0, code, KEYEVENTF_UNICODE), _key(0, code, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]
        else:
            out += [_key(code), _key(code, 0, KEYEVENTF_KEYUP)]
    return out


def chord_inputs(vks: Sequence[int]) -> list[INPUT]:
    """Press in order, release in reverse."""
    return [_key(v) for v in vks] + [_key(v, 0, KEYEVENTF_KEYUP) for v in reversed(vks)]


def decode_clipboard_text(raw: bytes) -> str:
    """UTF-16LE up to the first NUL, as stored in CF_UNICODETEXT."""
    text = raw.decode("utf-16-le", "replace")
    return text.split("\x00", 1)[0]


@dataclass
class _Dlls:
    user32: object
    kernel32: object
    advapi32: object


def _load_dlls() -> _Dlls:
    if sys.platform != "win32":
        raise WordInkError("Windows input and clipboard APIs are only available on Windows")
    from ctypes import windll  # noqa: PLC0415 - the only place that does

    u, k, a = windll.user32, windll.kernel32, windll.advapi32
    H = ctypes.c_void_p
    u.GetForegroundWindow.restype = H
    u.GetWindowThreadProcessId.argtypes = (H, ctypes.POINTER(wintypes.DWORD))
    u.SendInput.argtypes = (wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int)
    u.SendInput.restype = wintypes.UINT
    u.GetAsyncKeyState.argtypes = (ctypes.c_int,)
    u.GetAsyncKeyState.restype = ctypes.c_short
    u.OpenClipboard.argtypes = (H,)
    u.OpenClipboard.restype = wintypes.BOOL
    u.EmptyClipboard.restype = wintypes.BOOL
    u.CloseClipboard.restype = wintypes.BOOL
    u.GetClipboardData.argtypes = (wintypes.UINT,)
    u.GetClipboardData.restype = H
    u.SetClipboardData.argtypes = (wintypes.UINT, H)
    u.SetClipboardData.restype = H
    u.EnumClipboardFormats.argtypes = (wintypes.UINT,)
    u.EnumClipboardFormats.restype = wintypes.UINT
    u.IsClipboardFormatAvailable.argtypes = (wintypes.UINT,)
    u.IsClipboardFormatAvailable.restype = wintypes.BOOL
    u.RegisterClipboardFormatW.argtypes = (wintypes.LPCWSTR,)
    u.RegisterClipboardFormatW.restype = wintypes.UINT
    k.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    k.OpenProcess.restype = H
    k.CloseHandle.argtypes = (H,)
    k.GetCurrentProcess.restype = H
    k.QueryFullProcessImageNameW.argtypes = (H, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD))
    k.QueryFullProcessImageNameW.restype = wintypes.BOOL
    k.GlobalAlloc.argtypes = (wintypes.UINT, ctypes.c_size_t)
    k.GlobalAlloc.restype = H
    k.GlobalFree.argtypes = (H,)
    k.GlobalLock.argtypes = (H,)
    k.GlobalLock.restype = H
    k.GlobalUnlock.argtypes = (H,)
    k.GlobalSize.argtypes = (H,)
    k.GlobalSize.restype = ctypes.c_size_t
    a.OpenProcessToken.argtypes = (H, wintypes.DWORD, ctypes.POINTER(H))
    a.GetTokenInformation.argtypes = (H, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD))
    return _Dlls(u, k, a)


class Win32:
    """`InsertApi` over the real Windows APIs. Cheap to construct; loads the DLLs on first use."""

    def __init__(self) -> None:
        self._d: _Dlls | None = None
        self._exclude_ids: list[int] | None = None

    @property
    def d(self) -> _Dlls:
        if self._d is None:
            self._d = _load_dlls()
        return self._d

    # -- processes ------------------------------------------------------------------------------
    def _foreground_pid(self) -> int | None:
        u = self.d.user32
        hwnd = u.GetForegroundWindow()
        if not hwnd:
            return None
        pid = wintypes.DWORD(0)
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        return pid.value or None

    def foreground_process(self) -> str | None:
        pid = self._foreground_pid()
        if pid is None:
            return None
        k = self.d.kernel32
        handle = k.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
        if not handle:
            return None
        try:
            size = wintypes.DWORD(1024)
            buf = ctypes.create_unicode_buffer(size.value)
            if not k.QueryFullProcessImageNameW(handle, 0, buf, ctypes.byref(size)):
                return None
            return os.path.basename(buf.value).casefold()
        finally:
            k.CloseHandle(handle)

    def _elevated(self, process_handle) -> bool | None:
        a, k = self.d.advapi32, self.d.kernel32
        token = ctypes.c_void_p()
        if not a.OpenProcessToken(process_handle, TOKEN_QUERY, ctypes.byref(token)):
            return None
        try:
            elevation = wintypes.DWORD(0)
            got = wintypes.DWORD(0)
            ok = a.GetTokenInformation(token, TOKEN_ELEVATION, ctypes.byref(elevation), ctypes.sizeof(elevation), ctypes.byref(got))
            return bool(elevation.value) if ok else None
        finally:
            k.CloseHandle(token)

    def self_elevated(self) -> bool:
        return bool(self._elevated(self.d.kernel32.GetCurrentProcess()))

    def foreground_elevated(self) -> bool:
        """True when the foreground process runs elevated. A process we cannot even open for
        limited queries is assumed elevated (higher integrity), because UIPI would block us."""
        pid = self._foreground_pid()
        if pid is None:
            return False
        k = self.d.kernel32
        handle = k.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
        if not handle:
            return True
        try:
            result = self._elevated(handle)
        finally:
            k.CloseHandle(handle)
        return True if result is None else result

    # -- keyboard -------------------------------------------------------------------------------
    def _send(self, inputs: list[INPUT]) -> None:
        if not inputs:
            return
        arr = (INPUT * len(inputs))(*inputs)
        sent = self.d.user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT))
        if sent != len(inputs):
            raise OSError(f"SendInput delivered {sent} of {len(inputs)} events (blocked by a higher-integrity window?)")

    def send_events(self, events: Sequence[Event]) -> None:
        self._send(build_inputs(events))

    def send_chord(self, vks: Sequence[int]) -> None:
        self._send(chord_inputs(vks))

    def release_modifiers(self, wait: float) -> None:
        """Wait up to `wait` seconds for Ctrl, Alt and Shift to be physically up, then release stragglers
        so the target never sees the dictation hotkey's modifiers combined with our output."""
        u = self.d.user32
        deadline = time.monotonic() + wait
        while time.monotonic() < deadline and any(u.GetAsyncKeyState(vk) & 0x8000 for vk in _MODIFIER_VKS):
            time.sleep(0.01)
        down = [vk for vk in _MODIFIER_VKS if u.GetAsyncKeyState(vk) & 0x8000]
        self._send([_key(vk, 0, KEYEVENTF_KEYUP) for vk in down])

    # -- clipboard ------------------------------------------------------------------------------
    def _open_clipboard(self) -> None:
        if not self.d.user32.OpenClipboard(None):
            raise ClipboardBusy()

    def _exclusion_ids(self) -> list[int]:
        if self._exclude_ids is None:
            self._exclude_ids = [self.d.user32.RegisterClipboardFormatW(n) for n in EXCLUDE_FORMATS]
        return self._exclude_ids

    def clipboard_snapshot(self) -> ClipboardSnapshot:
        u, k = self.d.user32, self.d.kernel32
        ours = set(self._exclusion_ids())
        self._open_clipboard()
        try:
            text = None
            if u.IsClipboardFormatAvailable(CF_UNICODETEXT):
                handle = u.GetClipboardData(CF_UNICODETEXT)
                if handle:
                    ptr = k.GlobalLock(handle)
                    if ptr:
                        try:
                            text = decode_clipboard_text(ctypes.string_at(ptr, k.GlobalSize(handle)))
                        finally:
                            k.GlobalUnlock(handle)
            other, fmt = False, 0
            while True:
                fmt = u.EnumClipboardFormats(fmt)
                if not fmt:
                    break
                if fmt not in _TEXT_FORMATS and fmt not in ours:
                    other = True
            return ClipboardSnapshot(text, other)
        finally:
            u.CloseClipboard()

    def _put(self, fmt: int, data: bytes) -> None:
        k, u = self.d.kernel32, self.d.user32
        handle = k.GlobalAlloc(GMEM_MOVEABLE, len(data))
        if not handle:
            raise OSError("GlobalAlloc failed")
        ptr = k.GlobalLock(handle)
        if not ptr:
            k.GlobalFree(handle)
            raise OSError("GlobalLock failed")
        ctypes.memmove(ptr, data, len(data))
        k.GlobalUnlock(handle)
        if not u.SetClipboardData(fmt, handle):
            k.GlobalFree(handle)  # on success the system owns the memory
            raise OSError("SetClipboardData failed")

    def clipboard_set(self, text: str) -> None:
        u = self.d.user32
        ids = self._exclusion_ids()
        self._open_clipboard()
        try:
            u.EmptyClipboard()
            self._put(CF_UNICODETEXT, text.encode("utf-16-le") + b"\x00\x00")
            for fmt in ids:
                self._put(fmt, (0).to_bytes(4, "little"))
        finally:
            u.CloseClipboard()

    def clipboard_clear(self) -> None:
        self._open_clipboard()
        try:
            self.d.user32.EmptyClipboard()
        finally:
            self.d.user32.CloseClipboard()
