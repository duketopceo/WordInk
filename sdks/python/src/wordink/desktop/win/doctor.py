"""`wordink-dictd doctor` on Windows (plan W7c). Read-only; never prints a key, only whether one exists.

Every system query is a field of `Probes`, so the checks are tested on Linux with fakes. `real_probes()`
builds the Windows implementations lazily.
"""

from __future__ import annotations

import importlib.util
import re
import subprocess
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field

from ...client import WordInkError
from ..config import DictateConfig
from ..doctor import Check
from .hotkey import parse_chords

TASK_NAME = "WordInk Dictation"
LEGACY_PATTERN = re.compile(r"wordink\.main|groq-flow|groq_flow")


def _no_probe(*_a, **_k):
    raise WordInkError("probe unavailable")


@dataclass
class Probes:
    stored_key: Callable[[], bool] = _no_probe  # is GROQ_API_KEY in Credential Manager?
    input_device: Callable[[], str | None] = _no_probe  # default input device name, or None
    hotkey_clashes: Callable[[], list[str]] = _no_probe  # chords another app owns (RegisterHotKey probe)
    task_state: Callable[[], str | None] = _no_probe  # Task Scheduler state of our task, None if absent
    process_command_lines: Callable[[], list[str]] = _no_probe  # python processes' command lines
    daemon_state: Callable[[], str | None] = _no_probe  # status reply state, None if not running
    self_elevated: Callable[[], bool] = _no_probe
    modules: Callable[[str], bool] = field(default=lambda name: importlib.util.find_spec(name) is not None)


def run_checks(cfg: DictateConfig, env: Mapping[str, str], probes: Probes) -> list[Check]:
    out: list[Check] = []

    def add(name: str, status: str, detail: str, fix: str = "") -> None:
        out.append(Check(name, status, detail, fix))

    def probe(name: str, fn: Callable, on_error: str, fix: str = ""):
        try:
            return fn()
        except Exception as exc:  # a failing probe is a finding, not a crash
            add(name, "warn", f"{on_error} ({type(exc).__name__})", fix)
            return Ellipsis

    add("python", "ok" if sys.version_info >= (3, 11) else "fail", f"{sys.version_info.major}.{sys.version_info.minor}", "needs Python 3.11 or newer")

    gateway = env.get("WORDINK_GATEWAY_URL")
    if gateway:
        has = bool(env.get("WORDINK_DEVICE_TOKEN") or env.get("WORDINK_ADMIN_TOKEN"))
        add("credentials", "ok" if has else "fail", "gateway URL set, device token " + ("set" if has else "missing"), "set WORDINK_DEVICE_TOKEN in the environment")
    else:
        stored = probe("credentials", probes.stored_key, "could not read Credential Manager", "run `wordink-dictd set-key`")
        if stored is not Ellipsis:
            if env.get("GROQ_API_KEY"):
                add("credentials", "ok", "GROQ_API_KEY is set in the environment")
            elif stored:
                add("credentials", "ok", "a Groq key is stored in Windows Credential Manager")
            else:
                add("credentials", "fail", "no Groq key found", "run `wordink-dictd set-key` (hidden prompt) or set GROQ_API_KEY")

    device = probe("microphone", probes.input_device, "could not query audio devices", "install wordink with the 'windows' extra")
    if device is not Ellipsis:
        if device:
            add("microphone", "ok", f"default input: {device}")
        else:
            add("microphone", "fail", "no input device", "plug in a microphone and set it as default in Settings, System, Sound; check Settings, Privacy & security, Microphone")

    for mod, why in (("pystray", "tray icon"), ("PIL", "tray icon images"), ("keyring", "key storage")):
        found = probes.modules(mod)
        add(f"{mod} package", "ok" if found else "fail", "installed" if found else "missing", f"install wordink with the 'windows' extra ({why})")

    try:
        parse_chords(cfg.hold_bind, cfg.toggle_bind)
    except WordInkError as exc:
        add("hotkeys", "fail", str(exc), "fix [hotkey] in dictate.toml")
    else:
        running = probe("daemon", probes.daemon_state, "could not reach the daemon")
        clashes = [] if running not in (None, Ellipsis) else probe("hotkeys", probes.hotkey_clashes, "could not probe the hotkeys")
        if clashes is not Ellipsis:
            if clashes:
                add("hotkeys", "fail", f"{', '.join(clashes)} already registered by another application", "change [hotkey] in dictate.toml or close the other tool (AutoHotkey, PowerToys)")
            else:
                add("hotkeys", "ok", f"hold {cfg.hold_bind}, toggle {cfg.toggle_bind}" + (" (daemon running, not probed)" if running not in (None, Ellipsis) else ", free"))
        if running is not Ellipsis:
            if running:
                add("daemon", "ok", f"running, state {running}")
            else:
                add("daemon", "warn", "not running", "start it from Task Scheduler (or sign out and in), or run `wordink-dictd-tray`")

    task = probe("scheduled task", probes.task_state, "could not query Task Scheduler")
    if task is not Ellipsis:
        if task is None:
            add("scheduled task", "warn", f"'{TASK_NAME}' is not registered", "run install.ps1 to start dictation at logon")
        elif str(task).lower() == "disabled":
            add("scheduled task", "warn", "registered but disabled (the rollback script disables it)", "run install.ps1 -Switch to enable it again")
        else:
            add("scheduled task", "ok", f"registered, state {task}")

    lines = probe("legacy app", probes.process_command_lines, "could not list processes")
    if lines is not Ellipsis:
        legacy = [ln for ln in lines if LEGACY_PATTERN.search(ln)]
        if legacy:
            add("legacy app", "warn", "the legacy Groq Flow app is running", "two apps may fight over the hotkey; run install.ps1 -Switch to replace it")
        else:
            add("legacy app", "ok", "not running")

    elevated = probe("elevation", probes.self_elevated, "could not read the process token")
    if elevated is not Ellipsis:
        if elevated:
            add("elevation", "ok", "running as administrator: can type into elevated windows")
        else:
            add("elevation", "ok", "normal rights: elevated windows (Task Manager, admin terminals) get a clipboard fallback instead of typing")
    return out


# -- real probes -----------------------------------------------------------------------------------
def _run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, capture_output=True, text=True, timeout=10, check=False)


def real_probes(cfg: DictateConfig) -> Probes:
    import asyncio

    from .secrets import KeyStore
    from .transport import LoopbackTransport

    def stored_key() -> bool:
        return KeyStore().get() is not None

    def input_device() -> str | None:
        import sounddevice as sd

        try:
            return str(sd.query_devices(kind="input")["name"])
        except Exception:
            return None

    def hotkey_clashes() -> list[str]:
        from .hotkey import parse_chords as pc
        from .hook import probe_clashes
        from .win32 import Win32Hook

        return probe_clashes(Win32Hook(), *pc(cfg.hold_bind, cfg.toggle_bind))

    def task_state() -> str | None:
        done = _run(["powershell", "-NoProfile", "-Command", f"(Get-ScheduledTask -TaskName '{TASK_NAME}' -ErrorAction SilentlyContinue).State"])
        text = done.stdout.strip()
        return text or None

    def process_command_lines() -> list[str]:
        done = _run(["powershell", "-NoProfile", "-Command",
                     "Get-CimInstance Win32_Process -Filter \"Name LIKE 'python%'\" | ForEach-Object { $_.CommandLine }"])
        return [ln for ln in done.stdout.splitlines() if ln.strip()]

    def daemon_state() -> str | None:
        t = LoopbackTransport(cfg.runtime_dir)
        if not t.alive():
            return None
        return str(asyncio.run(t.request("status")).get("state", "?"))

    def self_elevated() -> bool:
        from .win32 import Win32

        return Win32().self_elevated()

    return Probes(stored_key, input_device, hotkey_clashes, task_state, process_command_lines, daemon_state, self_elevated)
