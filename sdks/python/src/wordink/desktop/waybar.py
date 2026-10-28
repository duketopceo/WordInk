"""The waybar module: turns the daemon's state file into waybar's JSON, once or as a stream."""

from __future__ import annotations

import json
import socket
import sys
import time
from collections.abc import Callable
from pathlib import Path
from typing import TextIO

from .indicator import StateFile

ICONS = {"idle": "\uf130", "recording": "\uf111", "transcribing": "\uf110", "error": "\uf071", "off": "\uf131"}
LABELS = {
    "idle": "WordInk ready",
    "recording": "Listening…",
    "transcribing": "Transcribing…",
    "error": "WordInk error",
    "off": "wordink-dictd is not running",
}


def daemon_alive(socket_path: Path) -> bool:
    probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    probe.settimeout(0.5)
    try:
        probe.connect(str(socket_path))
    except OSError:
        return False
    finally:
        probe.close()
    return True


def payload(state: dict, alive: bool) -> dict:
    """waybar `return-type: json` object. `class` drives the CSS; the tooltip carries error detail."""
    name = state.get("state", "idle") if alive else "off"
    if name not in ICONS:
        name = "error"
    tooltip = LABELS[name]
    detail = state.get("detail")
    if detail:
        tooltip = f"{tooltip}\n{detail}"
    return {"text": ICONS[name], "class": name, "alt": name, "tooltip": tooltip}


def snapshot(state_path: Path, socket_path: Path) -> dict:
    return payload(StateFile(state_path).read(), daemon_alive(socket_path))


def follow(
    state_path: Path,
    socket_path: Path,
    out: TextIO = sys.stdout,
    *,
    interval: float = 0.25,
    should_stop: Callable[[], bool] = lambda: False,
    sleep: Callable[[float], None] = time.sleep,
) -> None:
    """Print a JSON line whenever the state changes (waybar `exec` without `interval`)."""
    last: dict | None = None
    while not should_stop():
        current = snapshot(state_path, socket_path)
        if current != last:
            out.write(json.dumps(current, ensure_ascii=False) + "\n")
            out.flush()
            last = current
        sleep(interval)
