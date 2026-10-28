"""Win32Hook's callback logic against a fake user32 (plan W4c). The real hook is exercised on Windows by W10."""

from __future__ import annotations

import ctypes
import threading
import types

import pytest

from wordink.client import WordInkError
from wordink.desktop.interfaces import HotkeySource
from wordink.desktop.win import win32
from wordink.desktop.win.hook import HookApi
from wordink.desktop.win.hotkey import parse_chord


class FakeUser32:
    def __init__(self, hook_ok=True, hotkeys_ok=True) -> None:
        self.hook_ok, self.hotkeys_ok = hook_ok, hotkeys_ok
        self.proc = None
        self.unhooked = False
        self.quit = threading.Event()
        self.posted: list[tuple] = []
        self.registered: list[tuple] = []
        self.unregistered: list[int] = []
        self.next_calls: list[tuple] = []

    def SetWindowsHookExW(self, kind, proc, module, tid):
        self.proc = proc
        return 1 if self.hook_ok else 0

    def UnhookWindowsHookEx(self, hook):
        self.unhooked = True

    def CallNextHookEx(self, hook, n, w, l):
        self.next_calls.append((n, w))
        return 77

    def GetMessageW(self, msg, hwnd, lo, hi):
        self.quit.wait(3)
        return 0

    def TranslateMessage(self, m): ...
    def DispatchMessageW(self, m): ...

    def PostThreadMessageW(self, tid, msg, w, l):
        self.posted.append((tid, msg))
        self.quit.set()

    def RegisterHotKey(self, hwnd, ident, mods, vk):
        self.registered.append((ident, mods, vk))
        return self.hotkeys_ok

    def UnregisterHotKey(self, hwnd, ident):
        self.unregistered.append(ident)


def make(user32=None):
    u = user32 or FakeUser32()
    k = types.SimpleNamespace(GetCurrentThreadId=lambda: 4321, GetModuleHandleW=lambda n: 1)
    dlls = win32._Dlls(u, k, None)
    return u, win32.Win32Hook(dlls, functype=ctypes.CFUNCTYPE)


def run(hook, on_key):
    ready = threading.Event()
    t = threading.Thread(target=hook.run_hook, args=(on_key, ready.set), daemon=True)
    t.start()
    assert ready.wait(2)
    return t


def call(u, vk, msg, flags=0):
    kb = win32.KBDLLHOOKSTRUCT(vkCode=vk, scanCode=0, flags=flags, time=0, dwExtraInfo=0)
    return u.proc(win32.HC_ACTION, msg, ctypes.addressof(kb))


def test_it_satisfies_the_hook_api_and_installs_a_low_level_hook():
    u, hook = make()
    assert all(callable(getattr(win32.Win32Hook, n)) for n in vars(HookApi) if not n.startswith("_"))
    t = run(hook, lambda *a: False)
    assert u.proc is not None
    hook.stop_hook()
    t.join(2)
    assert not t.is_alive() and u.unhooked and u.posted == [(4321, win32.WM_QUIT)]


def test_a_swallowed_key_returns_1_and_others_chain_to_the_next_hook():
    u, hook = make()
    seen: list[tuple] = []

    def on_key(vk, down, injected):
        seen.append((vk, down, injected))
        return vk == ord("D")

    t = run(hook, on_key)
    assert call(u, ord("D"), win32.WM_KEYDOWN) == 1
    assert call(u, ord("A"), win32.WM_SYSKEYDOWN) == 77
    assert call(u, ord("D"), 0x0101) == 1  # WM_KEYUP
    assert seen == [(ord("D"), True, False), (ord("A"), True, False), (ord("D"), False, False)]
    assert u.next_calls == [(win32.HC_ACTION, win32.WM_SYSKEYDOWN)]
    hook.stop_hook()
    t.join(2)


def test_the_injected_flag_is_passed_through():
    u, hook = make()
    seen = []
    t = run(hook, lambda vk, down, inj: seen.append(inj) or False)
    call(u, 65, win32.WM_KEYDOWN, win32.LLKHF_INJECTED)
    call(u, 65, win32.WM_KEYDOWN, 0)
    assert seen == [True, False]
    hook.stop_hook()
    t.join(2)


def test_a_negative_ncode_is_passed_on_untouched_and_a_raising_callback_never_unwinds():
    u, hook = make()

    def boom(*a):
        raise RuntimeError("bug")

    t = run(hook, boom)
    assert u.proc(-1, 0, 0) == 77  # n_code < 0: must call the next hook without reading lParam
    assert call(u, 65, win32.WM_KEYDOWN) == 77  # exception swallowed, key passes through
    hook.stop_hook()
    t.join(2)


def test_a_refused_hook_raises_oserror_and_leaves_no_thread_id():
    u, hook = make(FakeUser32(hook_ok=False))
    with pytest.raises(OSError, match="refused"):
        hook.run_hook(lambda *a: False, lambda: None)
    hook.stop_hook()  # safe with nothing running
    assert u.posted == []


def test_probe_registers_and_releases_at_once():
    u, hook = make()
    assert hook.probe_hotkey(parse_chord("Ctrl+Alt+D")) is True
    ident, mods, vk = u.registered[0]
    assert mods == win32.MOD_CONTROL | win32.MOD_ALT | win32.MOD_NOREPEAT and vk == ord("D")
    assert u.unregistered == [ident]
    u2, hook2 = make(FakeUser32(hotkeys_ok=False))
    assert hook2.probe_hotkey(parse_chord("Ctrl+Alt+D")) is False and u2.unregistered == []


def test_hotkey_modifier_flags():
    assert win32.hotkey_modifiers(parse_chord("Shift+F9")) == win32.MOD_SHIFT | win32.MOD_NOREPEAT
    assert win32.hotkey_modifiers(parse_chord("Ctrl+Alt+Shift+Q")) == 7 | win32.MOD_NOREPEAT


def test_the_fallback_is_a_hotkey_source_and_reports_a_clash(monkeypatch):
    u = FakeUser32(hotkeys_ok=False)
    k = types.SimpleNamespace(GetCurrentThreadId=lambda: 9, GetModuleHandleW=lambda n: 1)
    src = win32.RegisterHotKeySource(parse_chord("Ctrl+Alt+D"), parse_chord("Ctrl+Alt+S"), win32._Dlls(u, k, None))
    assert isinstance(src, HotkeySource)
    with pytest.raises(WordInkError, match="already uses Alt\\+Ctrl\\+D, Alt\\+Ctrl\\+S"):
        src.start(lambda: None, lambda: None, lambda: None)


def test_off_windows_the_real_hook_fails_clearly():
    import sys

    if sys.platform == "win32":
        pytest.skip("real Windows")
    with pytest.raises(WordInkError, match="only available on Windows"):
        win32.Win32Hook().run_hook(lambda *a: False, lambda: None)
