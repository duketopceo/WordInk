"""Shared fixtures for the dictd tests: a fake microphone process and short socket paths."""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

#: A "microphone": streams 16 kHz s16 mono silence in real time until it is terminated.
FAKE_MIC = (
    sys.executable,
    "-u",
    "-c",
    "import sys,time\n"
    "chunk=b'\\x01\\x00'*800\n"  # 50 ms of samples
    "while True:\n"
    "    sys.stdout.buffer.write(chunk); sys.stdout.buffer.flush(); time.sleep(0.05)\n",
)

#: A capture tool that dies at once, like a missing device.
BROKEN_MIC = (sys.executable, "-c", "import sys; sys.exit(3)")


def short_tmp() -> Path:
    """AF_UNIX paths top out near 108 bytes; pytest's tmp_path can exceed that."""
    return Path(tempfile.mkdtemp(prefix="wdk-", dir="/tmp"))


async def wait_until(predicate, timeout: float = 10.0, interval: float = 0.02) -> None:
    """Poll `predicate()` until it is truthy; fail loudly instead of guessing a sleep length."""
    import asyncio

    deadline = asyncio.get_running_loop().time() + timeout
    while not predicate():
        if asyncio.get_running_loop().time() > deadline:
            raise AssertionError("timed out waiting for condition")
        await asyncio.sleep(interval)


async def wait_for_state(cfg, state: str, timeout: float = 10.0) -> None:
    from wordink.desktop.indicator import StateFile

    await wait_until(lambda: StateFile(cfg.state_path).read().get("state") == state, timeout)


async def captured(daemon, seconds: float = 0.4, timeout: float = 10.0) -> None:
    """Wait until the daemon's recorder has really heard `seconds` of audio."""
    await wait_until(lambda: daemon.recorder.captured_seconds >= seconds, timeout)
