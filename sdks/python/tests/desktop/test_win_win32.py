"""The ctypes layer's pure parts, and the rule that nothing touches windll at import (plan W5b)."""

from __future__ import annotations

import ctypes
import importlib
import pathlib
import re
import sys

import pytest

from wordink.client import WordInkError
from wordink.desktop.win import win32
from wordink.desktop.win.insert import InsertApi

SRC = pathlib.Path(win32.__file__).parent


def test_importing_the_layer_loads_no_dll():
    assert "windll" not in vars(win32)
    mod = importlib.reload(win32)
    assert mod.Win32()._d is None  # construction is free


def test_only_win32_py_mentions_windll():
    offenders = [
        p.name for p in SRC.glob("*.py") if p.name != "win32.py" and re.search(r"\bwindll\b|\bWinDLL\b|\boledll\b", p.read_text())
    ]
    assert offenders == []


def test_windll_is_fetched_only_inside_the_loader():
    text = (SRC / "win32.py").read_text()
    top_level = [ln for ln in text.splitlines() if ln.startswith(("from ctypes import windll", "import windll"))]
    assert top_level == []  # the one import is indented inside _load_dlls


@pytest.mark.skipif(sys.platform == "win32", reason="checks the off-Windows error")
def test_calls_off_windows_fail_clearly():
    with pytest.raises(WordInkError, match="only available on Windows"):
        win32.Win32().foreground_process()


def test_win32_satisfies_the_insert_api_shape():
    for name in InsertApi.__dict__:
        if not name.startswith("_"):
            assert callable(getattr(win32.Win32, name)), name


@pytest.mark.skipif(sys.platform != "win32" or ctypes.sizeof(ctypes.c_void_p) != 8, reason="wintypes DWORD is 8 bytes off Windows; the real layout is checked by the Windows CI job (W9)")
def test_input_struct_layout_matches_windows_x64():
    assert ctypes.sizeof(win32.INPUT) == 40
    assert ctypes.sizeof(win32.KEYBDINPUT) == 24
    assert win32.INPUT.ki.offset == 8


def test_unicode_units_become_scan_code_down_up_pairs():
    inputs = win32.build_inputs([("unit", 0xD83D), ("unit", 0xDE00)])
    assert len(inputs) == 4
    flags = [i.ki.dwFlags for i in inputs]
    assert flags == [4, 6, 4, 6]  # UNICODE, UNICODE|KEYUP, ...
    assert [i.ki.wScan for i in inputs] == [0xD83D, 0xD83D, 0xDE00, 0xDE00]
    assert all(i.type == 1 and i.ki.wVk == 0 for i in inputs)


def test_virtual_keys_use_wvk_and_enter_has_both_edges():
    a, b = win32.build_inputs([("vk", 0x0D)])
    assert (a.ki.wVk, a.ki.dwFlags) == (0x0D, 0) and (b.ki.wVk, b.ki.dwFlags) == (0x0D, 2)


def test_chord_presses_in_order_and_releases_in_reverse():
    seq = [(i.ki.wVk, i.ki.dwFlags) for i in win32.chord_inputs([0x11, 0x10, 0x56])]
    assert seq == [(0x11, 0), (0x10, 0), (0x56, 0), (0x56, 2), (0x10, 2), (0x11, 2)]


def test_clipboard_text_decodes_up_to_the_first_nul():
    raw = "héllo 😀".encode("utf-16-le") + b"\x00\x00junk"
    assert win32.decode_clipboard_text(raw) == "héllo 😀"
    assert win32.decode_clipboard_text(b"") == ""


def test_exclusion_formats_are_the_documented_three():
    assert set(win32.EXCLUDE_FORMATS) == {
        "ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard",
    }
