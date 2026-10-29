"""`wordink-dictd doctor` on macOS (plan M6). Never prints a key, only whether one exists.

Read-only except one brief probe: when the daemon is not running, it registers the hotkeys for a moment
to see whether macOS or another app already owns them, then releases them.

Every system query is a field of `Probes`, so the checks are tested on Linux with fakes.
`real_probes()` builds the macOS implementations lazily. None of it has run on a Mac yet (plan M9).
"""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field

from ...client import WordInkError
from ..config import DictateConfig
from ..doctor import Check
from .hotkey import parse_chords

LAUNCH_LABEL = "io.wordink.dictd"


def _no_probe(*_a, **_k):
    raise WordInkError("probe unavailable")


@dataclass
class Probes:
    mac_version: Callable[[], tuple[str, str]] = _no_probe  # (macOS version, machine such as arm64)
    stored_key: Callable[[], bool] = _no_probe  # is GROQ_API_KEY in the Keychain?
    input_device: Callable[[], str | None] = _no_probe  # default input device name, or None
    microphone_access: Callable[[], str | None] = _no_probe  # granted | denied | undetermined | None (unknown)
    accessibility_trusted: Callable[[], bool] = _no_probe
    secure_input: Callable[[], bool] = _no_probe
    hotkey_clashes: Callable[[], list[str]] = _no_probe  # chords macOS or another app owns
    launch_agent_state: Callable[[], str | None] = _no_probe  # launchctl state of our agent, None if absent
    daemon_state: Callable[[], str | None] = _no_probe  # status reply state, None if not running
    permission_holder: Callable[[], str] = _no_probe  # the executable the user grants permissions to
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

    v = probe("macOS", probes.mac_version, "could not read the macOS version")
    if v is not Ellipsis:
        version, machine = v
        try:
            major = int(str(version).split(".")[0])
        except ValueError:
            major = 0
        if major and major < 14:
            add("macOS", "warn", f"{version} ({machine})", "WordInk is built and tested for macOS 14 or newer")
        else:
            add("macOS", "ok", f"{version} ({machine})")
    add("python", "ok" if sys.version_info >= (3, 11) else "fail", f"{sys.version_info.major}.{sys.version_info.minor}", "needs Python 3.11 or newer")

    gateway = env.get("WORDINK_GATEWAY_URL")
    if gateway:
        has = bool(env.get("WORDINK_DEVICE_TOKEN") or env.get("WORDINK_ADMIN_TOKEN"))
        add("credentials", "ok" if has else "fail", "gateway URL set, device token " + ("set" if has else "missing"), "set WORDINK_DEVICE_TOKEN in the environment")
    else:
        stored = probe("credentials", probes.stored_key, "could not read the Keychain", "run `wordink-dictd set-key`")
        if stored is not Ellipsis:
            if env.get("GROQ_API_KEY"):
                add("credentials", "ok", "GROQ_API_KEY is set in the environment")
            elif stored:
                add("credentials", "ok", "key found in the macOS Keychain")
            else:
                add("credentials", "fail", "no Groq key", "run `wordink-dictd set-key` (stored in the Keychain)")

    device = probe("microphone", probes.input_device, "could not list input devices", "check System Settings, Sound, Input")
    if device is not Ellipsis:
        if device:
            add("microphone", "ok", f"default input: {device}")
        else:
            add("microphone", "fail", "no input device", "plug one in or choose an input in System Settings, Sound, Input")
    access = probe("microphone access", probes.microphone_access, "could not read the Microphone permission")
    if access is not Ellipsis:
        if access == "granted":
            add("microphone access", "ok", "allowed")
        elif access == "denied":
            add("microphone access", "fail", "blocked", "turn it on in System Settings, Privacy & Security, Microphone")
        elif access == "undetermined":
            add("microphone access", "warn", "not asked yet: macOS asks on the first dictation", "choose Allow when it asks")
        else:
            add("microphone access", "warn", "cannot be read from here", "if dictation records silence, check System Settings, Privacy & Security, Microphone")

    holder = probe("permission holder", probes.permission_holder, "could not resolve the program")
    holder_note = f" for {holder}" if holder is not Ellipsis else ""
    trusted = probe("accessibility", probes.accessibility_trusted, "could not read the Accessibility permission")
    if trusted is not Ellipsis:
        if trusted:
            add("accessibility", "ok", "allowed: WordInk can type into other apps")
        else:
            add("accessibility", "fail", f"not allowed{holder_note}: text is copied to the pasteboard instead of typed", f"turn it on in System Settings, Privacy & Security, Accessibility{holder_note}")
    if holder is not Ellipsis:
        add("permission holder", "ok", f"grants go to {holder}")
    add("input monitoring", "ok", "not needed: hotkeys use RegisterEventHotKey, which asks for no Input Monitoring")

    secure = probe("secure input", probes.secure_input, "could not read secure input state")
    if secure is not Ellipsis:
        if secure:
            add("secure input", "warn", "on right now (a password field or a stuck app): typing falls back to the pasteboard", "close the password field, or quit the app that left it on")
        else:
            add("secure input", "ok", "off")

    try:
        parse_chords(cfg.hold_bind, cfg.toggle_bind)
    except WordInkError as exc:
        add("hotkeys", "fail", str(exc), "fix [hotkey] in dictate.toml")
    else:
        running = probe("daemon", probes.daemon_state, "could not reach the daemon")
        clashes = [] if running not in (None, Ellipsis) else probe("hotkeys", probes.hotkey_clashes, "could not probe the hotkeys")
        if clashes is not Ellipsis:
            if clashes:
                add("hotkeys", "fail", f"{', '.join(clashes)} already in use by macOS or another app", "change [hotkey] in dictate.toml")
            else:
                add("hotkeys", "ok", f"hold {cfg.hold_bind}, toggle {cfg.toggle_bind}" + (" (daemon running, not probed)" if running not in (None, Ellipsis) else ", free"))
        if running is not Ellipsis:
            if running:
                add("daemon", "ok", f"running, state {running}")
            else:
                add("daemon", "warn", "not running", "log out and in, or run `wordink-dictd-mac`")

    agent = probe("launch agent", probes.launch_agent_state, "could not query launchctl")
    if agent is not Ellipsis:
        if agent is None:
            add("launch agent", "warn", f"'{LAUNCH_LABEL}' is not loaded", "run the macOS installer to start dictation at login")
        else:
            add("launch agent", "ok", f"loaded, state {agent}")

    for mod, why in (("rumps", "menu bar"), ("keyring", "key storage"), ("sounddevice", "microphone"), ("Quartz", "typing and hotkeys"), ("AppKit", "pasteboard")):
        found = probes.modules(mod)
        add(f"{mod} package", "ok" if found else "fail", "installed" if found else "missing", f"install wordink with the 'macos' extra ({why})")
    return out


# -- real probes -----------------------------------------------------------------------------------
def _run(cmd: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, capture_output=True, text=True, timeout=10, check=False)


def real_probes(cfg: DictateConfig) -> Probes:
    import asyncio
    import platform

    from ..transport import UnixTransport
    from . import appkit, quartz
    from .secrets import KeyStore

    def mac_version() -> tuple[str, str]:
        return platform.mac_ver()[0] or "unknown", platform.machine()

    def stored_key() -> bool:
        return KeyStore().get() is not None

    def input_device() -> str | None:
        import sounddevice as sd

        try:
            return str(sd.query_devices(kind="input")["name"])
        except Exception:
            return None

    def hotkey_clashes() -> list[str]:
        from .hotkey import HotkeyClash, parse_chords as pc
        from .hotkey import HotkeySource

        hold, toggle = pc(cfg.hold_bind, cfg.toggle_bind)
        src = HotkeySource(hold, toggle, quartz.CarbonApi())
        try:
            src.start(lambda: None, lambda: None, lambda: None)
        except HotkeyClash as exc:
            return [str(exc)]
        src.stop()
        return []

    def launch_agent_state() -> str | None:
        done = _run(["launchctl", "print", f"gui/{os.getuid()}/{LAUNCH_LABEL}"])
        if done.returncode != 0:
            return None
        for line in done.stdout.splitlines():
            if line.strip().startswith("state ="):
                return line.split("=", 1)[1].strip()
        return "loaded"

    def daemon_state() -> str | None:
        t = UnixTransport(cfg.socket_path)
        if not t.alive():
            return None
        return str(asyncio.run(t.request("status")).get("state", "?"))

    def permission_holder() -> str:
        return os.path.realpath(sys.executable)

    return Probes(
        mac_version, stored_key, input_device, appkit.microphone_access, quartz.accessibility_trusted,
        quartz.secure_input_enabled, hotkey_clashes, launch_agent_state, daemon_state, permission_holder,
    )  # fmt: skip
