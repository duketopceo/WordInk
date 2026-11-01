"""Put dictated text into the focused window: wtype, then ydotool, then the clipboard.

Terminals get a paste (clipboard + Ctrl+Shift+V, then the old clipboard back) because typing
into them key by key is slow and drops characters. Password managers are never typed into.
"""

from __future__ import annotations

import json
import logging
import shutil
import subprocess
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass, field

from ..client import WordInkError

log = logging.getLogger("wordink.dictd")

DEFAULT_ORDER: tuple[str, ...] = ("wtype", "ydotool")
DEFAULT_TERMINALS: tuple[str, ...] = (
    "alacritty", "kitty", "foot", "footclient", "ghostty", "com.mitchellh.ghostty", "wezterm",
    "org.wezfurlong.wezterm", "xterm", "konsole", "gnome-terminal", "org.gnome.terminal", "tilix", "st",
)
DEFAULT_DENY: tuple[str, ...] = (
    "1password", "bitwarden", "keepassxc", "org.keepassxc.keepassxc", "keepass", "proton pass", "enpass",
)
RUN_TIMEOUT = 5.0

# ydotool key codes (linux input-event-codes): Ctrl=29, Shift=42, V=47
_YDOTOOL_PASTE = ["29:1", "42:1", "47:1", "47:0", "42:0", "29:0"]


@dataclass(frozen=True)
class InsertResult:
    #: typed | pasted | clipboard | skipped
    method: str
    detail: str = ""


@dataclass(frozen=True)
class InjectConfig:
    order: tuple[str, ...] = DEFAULT_ORDER
    terminals: frozenset[str] = field(default_factory=lambda: frozenset(DEFAULT_TERMINALS))
    deny: frozenset[str] = field(default_factory=lambda: frozenset(DEFAULT_DENY))
    paste_delay: float = 0.15
    notify: bool = True


Runner = Callable[[Sequence[str], bytes | None], "subprocess.CompletedProcess[bytes]"]


def _run(cmd: Sequence[str], stdin: bytes | None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(list(cmd), input=stdin, capture_output=True, timeout=RUN_TIMEOUT, check=False)


def _run_serve(cmd: Sequence[str], stdin: bytes | None) -> subprocess.CompletedProcess[bytes]:
    # wl-copy forks a child that serves the selection until it is replaced; captured
    # pipes stay open in the child, so run() with capture_output never returns.
    proc = subprocess.Popen(
        list(cmd), stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    try:
        proc.communicate(input=stdin, timeout=RUN_TIMEOUT)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
        raise
    return subprocess.CompletedProcess(list(cmd), proc.returncode)


class Injector:
    def __init__(
        self,
        cfg: InjectConfig | None = None,
        *,
        path: str | None = None,
        runner: Runner = _run,
        serve_runner: Runner = _run_serve,
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self.cfg = cfg or InjectConfig()
        self._path = path
        self._runner = runner
        self._serve_runner = serve_runner
        self._sleep = sleep

    # -- plumbing ---------------------------------------------------------------------------
    def _which(self, name: str) -> str | None:
        return shutil.which(name, path=self._path)

    def _try(self, name: str, args: Sequence[str], stdin: bytes | None = None) -> bool:
        exe = self._which(name)
        if exe is None:
            return False
        runner = self._serve_runner if name == "wl-copy" else self._runner
        try:
            return runner([exe, *args], stdin).returncode == 0
        except (OSError, subprocess.TimeoutExpired) as exc:
            log.warning("%s failed: %s", name, type(exc).__name__)
            return False

    def _output(self, name: str, args: Sequence[str]) -> bytes | None:
        exe = self._which(name)
        if exe is None:
            return None
        try:
            done = self._runner([exe, *args], None)
        except (OSError, subprocess.TimeoutExpired):
            return None
        return done.stdout if done.returncode == 0 else None

    def active_class(self) -> str | None:
        raw = self._output("hyprctl", ["activewindow", "-j"])
        if not raw:
            return None
        try:
            data = json.loads(raw)
        except ValueError:
            return None
        cls = data.get("class") if isinstance(data, dict) else None
        return cls.casefold() if isinstance(cls, str) and cls else None

    def notify(self, message: str) -> None:
        if self.cfg.notify:
            self._try("notify-send", ["-a", "WordInk", "-t", "3000", "WordInk", message])

    # -- strategies -------------------------------------------------------------------------
    def _type(self, text: str) -> str | None:
        """The tool that typed `text`, or None."""
        for tool in self.cfg.order:
            if tool == "wtype" and self._try("wtype", ["--", text]):
                return "wtype"
            # ydotool types US-layout key codes only: never feed it anything it would mangle.
            if tool == "ydotool" and text.isascii() and self._try("ydotool", ["type", "--key-delay", "2", "--", text]):
                return "ydotool"
        return None

    def _send_paste_chord(self) -> bool:
        for tool in self.cfg.order:
            if tool == "wtype" and self._try("wtype", ["-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"]):
                return True
            if tool == "ydotool" and self._try("ydotool", ["key", *_YDOTOOL_PASTE]):
                return True
        return False

    def _paste(self, text: str) -> bool:
        """Clipboard + paste chord, then put the user's clipboard back. False if it could not paste."""
        if self._which("wl-copy") is None:
            return False
        previous = self._output("wl-paste", ["--no-newline"])
        if not self._try("wl-copy", [], text.encode()):
            return False
        pasted = self._send_paste_chord()
        if pasted:
            self._sleep(self.cfg.paste_delay)  # let the target app read the selection before we swap it back
            if previous:
                self._try("wl-copy", [], previous)
            else:
                self._try("wl-copy", ["--clear"])
        return pasted

    # -- entry point ------------------------------------------------------------------------
    def insert(self, text: str) -> InsertResult:
        if not text:
            return InsertResult("skipped", "empty")
        cls = self.active_class()
        if cls and any(d in cls for d in self.cfg.deny):
            self.notify("Skipped: a password manager is focused. Text is in `wordink-dictd last`.")
            return InsertResult("skipped", "denied window")
        if cls and cls in self.cfg.terminals and self._paste(text):
            return InsertResult("pasted", cls)
        tool = None if (cls and cls in self.cfg.terminals) else self._type(text)
        if tool:
            return InsertResult("typed", tool)
        if self._try("wl-copy", [], text.encode()):
            self.notify("Copied to the clipboard. Paste it with Ctrl+V.")
            return InsertResult("clipboard", "no typing tool worked")
        raise WordInkError("cannot insert text: install wtype (or ydotool, or wl-clipboard); the text is in `wordink-dictd last`")


def inject_config_from(raw: Mapping[str, object] | None) -> InjectConfig:
    """Build an InjectConfig from the `[insert]` table of dictate.toml (already validated)."""
    raw = raw or {}
    kw: dict[str, object] = {}
    if "order" in raw:
        kw["order"] = tuple(raw["order"])  # type: ignore[arg-type]
    if "terminals" in raw:
        kw["terminals"] = frozenset(str(t).casefold() for t in raw["terminals"])  # type: ignore[attr-defined]
    if "deny" in raw:
        kw["deny"] = frozenset(str(t).casefold() for t in raw["deny"])  # type: ignore[attr-defined]
    if "paste_delay" in raw:
        kw["paste_delay"] = float(raw["paste_delay"])  # type: ignore[arg-type]
    if "notify" in raw:
        kw["notify"] = bool(raw["notify"])
    return InjectConfig(**kw)  # type: ignore[arg-type]

