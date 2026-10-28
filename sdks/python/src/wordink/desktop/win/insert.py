"""Put dictated text into the focused Windows window (plan W5).

Strategy, in order: skip password managers; hand a clipboard-only result to the user when the
focused window is elevated and we are not (UIPI would drop our keystrokes); paste into terminals
and for long text; otherwise type with Unicode `SendInput`. Every OS call goes through `InsertApi`,
so the whole policy is tested on Linux with a fake.
"""

from __future__ import annotations

import logging
import threading
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass, field
from typing import Protocol

from ...client import WordInkError
from ..inject import InsertResult

log = logging.getLogger("wordink.dictd")

VK_RETURN, VK_CONTROL, VK_SHIFT, VK_V = 0x0D, 0x11, 0x10, 0x56

#: image names, lowercase
DEFAULT_TERMINALS: tuple[str, ...] = (
    "windowsterminal.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "conhost.exe", "wezterm-gui.exe",
    "alacritty.exe", "kitty.exe", "mintty.exe", "wt.exe", "ghostty.exe",
)  # fmt: skip
DEFAULT_DENY: tuple[str, ...] = (
    "1password", "bitwarden", "keepassxc", "keepass", "enpass", "protonpass", "dashlane", "lastpass",
)
CHUNK_UNITS = 50
LONG_TEXT_CHARS = 400
CLIPBOARD_RETRIES = 5

Event = tuple[str, int]  # ("unit", utf16 code unit) or ("vk", virtual-key code)


class ClipboardBusy(Exception):
    """Another process holds the clipboard (`OpenClipboard` failed). Retryable."""


@dataclass(frozen=True)
class ClipboardSnapshot:
    text: str | None  # current CF_UNICODETEXT, if any
    other_formats: bool  # anything besides text (images, files, rich data): we will not clobber it


class InsertApi(Protocol):
    def foreground_process(self) -> str | None: ...
    def foreground_elevated(self) -> bool: ...
    def self_elevated(self) -> bool: ...
    def release_modifiers(self, wait: float) -> None:
        """Send key-ups for Ctrl, Alt, Shift, then wait up to `wait` seconds for them to be physically up."""
        ...

    def send_events(self, events: Sequence[Event]) -> None: ...
    def send_chord(self, vks: Sequence[int]) -> None:
        """Press the keys in order, release in reverse."""
        ...

    def clipboard_snapshot(self) -> ClipboardSnapshot: ...
    def clipboard_set(self, text: str) -> None:
        """Set text, plus the formats that keep it out of Win+V history and the cloud clipboard."""
        ...

    def clipboard_clear(self) -> None: ...


@dataclass(frozen=True)
class WinInsertConfig:
    terminals: frozenset[str] = field(default_factory=lambda: frozenset(DEFAULT_TERMINALS))
    deny: frozenset[str] = field(default_factory=lambda: frozenset(DEFAULT_DENY))
    paste_delay: float = 0.25
    long_text: int = LONG_TEXT_CHARS
    notify: bool = True


def win_insert_config_from(raw: Mapping[str, object] | None) -> WinInsertConfig:
    """From the `[insert]` table of dictate.toml. Names are image names (`1Password.exe`) or fragments."""
    raw = raw or {}
    kw: dict[str, object] = {}
    for key in ("terminals", "deny"):
        if key in raw:
            kw[key] = frozenset(str(v).casefold() for v in raw[key])  # type: ignore[attr-defined]
    if "paste_delay" in raw:
        kw["paste_delay"] = float(raw["paste_delay"])  # type: ignore[arg-type]
    if "notify" in raw:
        kw["notify"] = bool(raw["notify"])
    return WinInsertConfig(**kw)  # type: ignore[arg-type]


def plan_events(text: str) -> list[Event]:
    """UTF-16 code units (surrogate pairs kept together), with every newline form as one Enter."""
    events: list[Event] = []
    i = 0
    while i < len(text):
        ch = text[i]
        if ch == "\r" and text[i + 1 : i + 2] == "\n":
            i += 1
            ch = "\n"
        if ch in "\r\n":
            events.append(("vk", VK_RETURN))
        else:
            raw = ch.encode("utf-16-le", "surrogatepass")
            events.extend(("unit", int.from_bytes(raw[j : j + 2], "little")) for j in range(0, len(raw), 2))
        i += 1
    return events


def _chunks(events: list[Event], size: int) -> list[list[Event]]:
    """Batches of at most `size` events that never split a surrogate pair."""
    out: list[list[Event]] = []
    cur: list[Event] = []
    for ev in events:
        if len(cur) >= size and not (cur[-1][0] == "unit" and 0xD800 <= cur[-1][1] <= 0xDBFF):
            out.append(cur)
            cur = []
        cur.append(ev)
    if cur:
        out.append(cur)
    return out


Scheduler = Callable[[float, Callable[[], None]], object]


def _timer(delay: float, fn: Callable[[], None]) -> object:
    t = threading.Timer(delay, fn)
    t.daemon = True
    t.start()
    return t


class WindowsInserter:
    def __init__(
        self,
        api: InsertApi,
        cfg: WinInsertConfig | None = None,
        *,
        notifier: Callable[[str], object] | None = None,
        sleep: Callable[[float], None] = time.sleep,
        schedule: Scheduler = _timer,
    ) -> None:
        self.api = api
        self.cfg = cfg or WinInsertConfig()
        self._notifier = notifier
        self._sleep = sleep
        self._schedule = schedule
        self._lock = threading.RLock()
        self._generation = 0
        self._pending_restore: ClipboardSnapshot | None = None  # the user's clipboard, while one of ours is on it
        self._our_text: str | None = None

    def notify(self, message: str) -> None:
        if self.cfg.notify and self._notifier is not None:
            self._notifier(message)

    # -- clipboard ------------------------------------------------------------------------------
    def _retry(self, fn: Callable[[], object]) -> object:
        for attempt in range(CLIPBOARD_RETRIES):
            try:
                return fn()
            except ClipboardBusy:
                if attempt == CLIPBOARD_RETRIES - 1:
                    raise
                self._sleep(0.02 * (attempt + 1))
        raise AssertionError("unreachable")

    def _restore(self, generation: int) -> None:
        """Put the user's clipboard back, unless a newer dictation owns it or the user copied something."""
        with self._lock:
            if generation != self._generation or self._pending_restore is None:
                return  # a later dictation took over; it restores the original instead
            saved, ours = self._pending_restore, self._our_text
            self._pending_restore = self._our_text = None
            try:
                now = self._retry(self.api.clipboard_snapshot)
                if now.text != ours or now.other_formats:  # type: ignore[attr-defined]
                    return  # the user (or another app) changed it: leave theirs alone
                if saved.text:
                    self._retry(lambda: self.api.clipboard_set(saved.text))  # type: ignore[arg-type]
                else:
                    self._retry(self.api.clipboard_clear)
            except ClipboardBusy:
                log.warning("could not restore the clipboard: it stayed busy")

    def _paste(self, text: str, chord: Sequence[int]) -> bool:
        """Clipboard + paste chord, restore later. False when it must not or cannot paste."""
        with self._lock:
            try:
                snap = self._retry(self.api.clipboard_snapshot)
                if snap.other_formats:  # type: ignore[attr-defined]
                    return False  # an image or files are on the clipboard: do not destroy them
                self._generation += 1
                generation = self._generation
                if self._pending_restore is None:  # else the clipboard holds our previous text, not the user's
                    self._pending_restore = snap  # type: ignore[assignment]
                self._our_text = text
                self._retry(lambda: self.api.clipboard_set(text))
                self.api.release_modifiers(0.3)
                self.api.send_chord(chord)
            except ClipboardBusy:
                self._pending_restore = self._our_text = None
                return False
            except Exception as exc:  # SendInput refused the paste chord: put the clipboard back, then type
                log.warning("paste chord failed: %s", type(exc).__name__)
                self._restore(generation)
                return False
        self._schedule(self.cfg.paste_delay, lambda: self._restore(generation))
        return True

    # -- typing ---------------------------------------------------------------------------------
    def _type(self, text: str) -> None:
        self.api.release_modifiers(0.3)
        for batch in _chunks(plan_events(text), CHUNK_UNITS):
            self.api.send_events(batch)
            self._sleep(0.002)

    def _hand_over(self, text: str, why: str) -> InsertResult:
        with self._lock:
            self._generation += 1  # cancel any pending restore: this text must stay on the clipboard
            self._pending_restore = self._our_text = None
            try:
                self._retry(lambda: self.api.clipboard_set(text))
            except ClipboardBusy as exc:
                raise WordInkError("cannot insert text: the clipboard is busy; the text is in `wordink-dictd last`") from exc
        self.notify(why)
        return InsertResult("clipboard", why)

    # -- entry point ----------------------------------------------------------------------------
    def insert(self, text: str) -> InsertResult:
        if not text:
            return InsertResult("skipped", "empty")
        proc = (self.api.foreground_process() or "").casefold()
        if proc and any(d in proc for d in self.cfg.deny):
            self.notify("Skipped: a password manager is focused. Text is in `wordink-dictd last`.")
            return InsertResult("skipped", "denied window")
        if self.api.foreground_elevated() and not self.api.self_elevated():
            return self._hand_over(text, "Copied to the clipboard: that window runs as administrator. Paste it with Ctrl+V.")
        terminal = proc in self.cfg.terminals
        if terminal or len(text) > self.cfg.long_text:
            chord = (VK_CONTROL, VK_SHIFT, VK_V) if terminal else (VK_CONTROL, VK_V)
            if self._paste(text, chord):
                return InsertResult("pasted", proc or "long text")
        try:
            self._type(text)
        except Exception as exc:
            log.warning("SendInput failed: %s", type(exc).__name__)
            return self._hand_over(text, "Copied to the clipboard: typing failed. Paste it with Ctrl+V.")
        return InsertResult("typed", "SendInput")
