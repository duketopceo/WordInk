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
