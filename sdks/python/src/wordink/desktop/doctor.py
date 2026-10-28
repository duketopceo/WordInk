"""`wordink-dictd doctor`: read-only checks that say what is missing and how to fix it.

It never prints secret values (only whether a key is set), never writes anything, and
never restarts or edits other tools.
"""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import subprocess
import sys
from collections.abc import Callable, Mapping, Sequence
from dataclasses import asdict, dataclass

from .binds import normalize_bind
from .config import DictateConfig
from .daemon import send_command
from .recorder import BACKENDS
from .waybar import daemon_alive

Runner = Callable[[Sequence[str]], "subprocess.CompletedProcess[bytes]"]

# Hyprland's modmask bits, as reported by `hyprctl binds -j`.
_MOD_BITS = {"SHIFT": 1, "CTRL": 4, "ALT": 8, "MOD2": 16, "MOD3": 32, "SUPER": 64, "MOD5": 128}


@dataclass(frozen=True)
class Check:
    name: str
    #: ok | warn | fail
    status: str
    detail: str
    fix: str = ""


def _run(cmd: Sequence[str]) -> "subprocess.CompletedProcess[bytes]":
    return subprocess.run(list(cmd), capture_output=True, timeout=5, check=False)


def _bind_signature(bind: str) -> tuple[int, str]:
    parts = normalize_bind(bind).split("|")
    return sum(_MOD_BITS[m] for m in parts[:-1]), parts[-1]


def run_checks(
    cfg: DictateConfig,
    env: Mapping[str, str] | None = None,
    *,
    path: str | None = None,
    runner: Runner = _run,
) -> list[Check]:
    e = os.environ if env is None else env
    which = lambda name: shutil.which(name, path=path)  # noqa: E731
    out: list[Check] = []

    def add(name: str, status: str, detail: str, fix: str = "") -> None:
        out.append(Check(name, status, detail, fix))

    add("python", "ok" if sys.version_info >= (3, 11) else "fail", f"{sys.version_info.major}.{sys.version_info.minor}", "needs Python 3.11 or newer")

    capture = next((n for n, _ in BACKENDS if which(n)), None)
    add("microphone tool", "ok" if capture else "fail", capture or "none of pw-record, parecord, arecord", "install pipewire (pw-record)")

    typers = [t for t in ("wtype", "ydotool") if which(t)]
    add("typing tool", "ok" if typers else "warn", ", ".join(typers) or "neither wtype nor ydotool", "install wtype; without it dictation falls back to the clipboard")
    if "ydotool" in typers and "wtype" not in typers:
        if not os.access("/dev/uinput", os.W_OK):
            add("ydotool permissions", "warn", "/dev/uinput is not writable by this user", "add yourself to the input group or add a uinput udev rule, then run ydotoold")
        if not which("ydotoold"):
            add("ydotoold", "warn", "ydotoold is not installed", "install it; ydotool needs the daemon")
    for tool, why in (("wl-copy", "clipboard fallback and terminal paste"), ("wl-paste", "clipboard restore after a terminal paste")):
        add(tool, "ok" if which(tool) else "warn", "found" if which(tool) else "missing", f"install wl-clipboard ({why})")
    add("notify-send", "ok" if which("notify-send") else "warn", "found" if which("notify-send") else "missing", "install libnotify for error pop-ups")

    add("wayland session", "ok" if e.get("WAYLAND_DISPLAY") else "warn", "WAYLAND_DISPLAY is set" if e.get("WAYLAND_DISPLAY") else "WAYLAND_DISPLAY is not set",
        "run from the Hyprland session; for the service, make sure the session environment is imported into systemd --user")

    gateway = e.get("WORDINK_GATEWAY_URL")
    if gateway:
        has = bool(e.get("WORDINK_DEVICE_TOKEN") or e.get("WORDINK_ADMIN_TOKEN"))
        add("credentials", "ok" if has else "fail", "gateway URL set, device token " + ("set" if has else "missing"), "set WORDINK_DEVICE_TOKEN in the service's EnvironmentFile")
        if not e.get("GROQ_API_KEY"):
            add("llm cleanup key", "warn", "GROQ_API_KEY not set; cleanup will use the offline rules", "set GROQ_API_KEY to enable the LLM pass")
    else:
        has = bool(e.get("GROQ_API_KEY"))
        add("credentials", "ok" if has else "fail", "GROQ_API_KEY is " + ("set" if has else "not set"), "put GROQ_API_KEY=... in ~/.config/wordink/dictd.env (mode 0600)")

    hypr = which("hyprctl")
    if not hypr:
        add("hyprland binds", "warn", "hyprctl not found, cannot inspect binds", "run inside Hyprland")
    else:
        try:
            done = runner([hypr, "binds", "-j"])
            binds = json.loads(done.stdout) if done.returncode == 0 else None
        except (OSError, subprocess.TimeoutExpired, ValueError):
            binds = None
        if not isinstance(binds, list):
            add("hyprland binds", "warn", "could not read `hyprctl binds -j`", "run inside the Hyprland session")
        else:
            ours = [b for b in binds if isinstance(b, dict) and "wordink-dictd" in str(b.get("arg", ""))]
            for label, bind in (("hold", cfg.hold_bind), ("toggle", cfg.toggle_bind)):
                mask, key = _bind_signature(bind)
                same = [b for b in binds if isinstance(b, dict) and b.get("modmask") == mask and str(b.get("key", "")).upper() == key]
                foreign = [b for b in same if "wordink-dictd" not in str(b.get("arg", ""))]
                if foreign:
                    what = f"{foreign[0].get('dispatcher')} {foreign[0].get('arg')}".strip()
                    add(f"{label} bind", "fail", f"{bind} is already bound to: {what}", "choose another key in dictate.toml [hotkey] and re-run `wordink-dictd binds`")
                elif any(b in ours for b in same):
                    add(f"{label} bind", "ok", f"{bind} runs wordink-dictd")
                else:
                    add(f"{label} bind", "warn", f"{bind} is free but not installed yet", "add the lines from `wordink-dictd binds` to your Hyprland config")

    if daemon_alive(cfg.socket_path):
        try:
            state = asyncio.run(send_command(cfg.socket_path, "status")).get("state", "?")
            add("daemon", "ok", f"running, state {state}")
        except Exception as exc:  # report, do not crash the doctor
            add("daemon", "warn", f"socket exists but did not answer: {type(exc).__name__}", "restart the service")
    else:
        add("daemon", "warn", "not running", "systemctl --user enable --now wordink-dictd")
    return out


def render(checks: Sequence[Check]) -> str:
    mark = {"ok": "ok  ", "warn": "warn", "fail": "FAIL"}
    lines = []
    for c in checks:
        lines.append(f"[{mark[c.status]}] {c.name}: {c.detail}")
        if c.status != "ok" and c.fix:
            lines.append(f"       fix: {c.fix}")
    return "\n".join(lines)


def to_json(checks: Sequence[Check]) -> str:
    return json.dumps([asdict(c) for c in checks], indent=2)


def exit_code(checks: Sequence[Check]) -> int:
    return 1 if any(c.status == "fail" for c in checks) else 0
