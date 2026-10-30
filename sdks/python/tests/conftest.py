"""Shared pytest hooks.

`mac_hardware` tests need a real Mac with a logged-in desktop session. They are skipped unless
`WORDINK_MAC_HARDWARE=1` is set on macOS, so CI and Linux runs never touch real hardware.
"""

from __future__ import annotations

import os
import sys

import pytest


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    if sys.platform == "darwin" and os.environ.get("WORDINK_MAC_HARDWARE") == "1":
        return
    skip = pytest.mark.skip(reason="needs a real Mac desktop session: set WORDINK_MAC_HARDWARE=1 on macOS (plan step M9)")
    for item in items:
        if "mac_hardware" in item.keywords:
            item.add_marker(skip)
