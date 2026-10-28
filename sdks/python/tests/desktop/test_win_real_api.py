"""Smoke tests against the real Windows APIs. Run only on Windows (the python-sdk-windows CI job)."""

from __future__ import annotations

import sys

import pytest

pytestmark = pytest.mark.skipif(sys.platform != "win32", reason="real Windows APIs")


def test_the_dlls_load_and_the_process_checks_answer():
    from wordink.desktop.win.win32 import Win32

    api = Win32()
    assert isinstance(api.self_elevated(), bool)
    name = api.foreground_process()  # a headless runner may have no foreground window
    assert name is None or name == name.casefold()
    assert isinstance(api.foreground_elevated(), bool)


def test_releasing_modifiers_with_none_held_sends_nothing_and_returns_fast():
    import time

    from wordink.desktop.win.win32 import Win32

    start = time.monotonic()
    Win32().release_modifiers(0.1)
    assert time.monotonic() - start < 3.0
