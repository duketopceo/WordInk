"""Desktop notifications via notify-send. Fire and forget: a missing notifier never breaks dictation."""

from __future__ import annotations

import shutil
import subprocess


def notify(message: str, *, urgency: str = "normal", path: str | None = None) -> bool:
    exe = shutil.which("notify-send", path=path)
    if exe is None:
        return False
    try:
        subprocess.Popen(
            [exe, "-a", "WordInk", "-u", urgency, "-t", "5000", "WordInk", message[:300]],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        return False
    return True
