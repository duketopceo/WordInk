"""Put dictated text into the focused macOS app (plan M4).

Strategy, in order: skip password managers (by bundle id); when a secure field has focus or
Accessibility is not granted, copy to the pasteboard and notify (typing would be dropped); paste
into terminals and for long text; otherwise type Unicode key events in small chunks. Every OS
call goes through `InsertApi`, so the whole policy is tested on Linux with a fake. `NativeInsertApi`
at the bottom wires the real calls from `quartz.py` and `appkit.py`; it has not run on a Mac (M9).
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

KVK_RETURN = 0x24

#: bundle identifiers, exact (lowercase)
DEFAULT_TERMINALS: tuple[str, ...] = (
    "com.apple.terminal", "com.googlecode.iterm2", "dev.warp.warp-stable", "com.mitchellh.ghostty",
    "net.kovidgoyal.kitty", "org.alacritty", "com.github.wez.wezterm", "co.zeit.hyper", "io.alacritty",
)  # fmt: skip
#: fragments of a bundle id (lowercase); `com.1password.1password`, `com.bitwarden.desktop`, ...
DEFAULT_DENY: tuple[str, ...] = (
    "1password", "onepassword", "bitwarden", "keepassxc", "keepass", "enpass", "protonpass", "dashlane", "lastpass",
)
CHUNK_UNITS = 20  # UTF-16 units per Unicode key event (the CGEvent limit)
LONG_TEXT_CHARS = 200
PASTEBOARD_RETRIES = 3

Event = tuple[str, int]  # ("unit", utf16 code unit) or ("vk", virtual-key code)


class PasteboardBusy(Exception):
    """The pasteboard could not be read or written right now. Retryable."""


@dataclass(frozen=True)
class PasteboardSnapshot:
    text: str | None  # the current plain string, if any
    other_formats: bool  # anything besides text (images, files, rich data): we will not clobber it


class InsertApi(Protocol):
    def frontmost_bundle_id(self) -> str | None: ...
    def secure_input_enabled(self) -> bool:
        """True while a password field (or an app that forgot to turn it off) has secure input on."""
        ...

    def accessibility_trusted(self) -> bool: ...
    def release_modifiers(self, wait: float) -> None:
        """Wait up to `wait` seconds for Ctrl, Option, Shift and Cmd to be physically up."""
        ...

    def send_events(self, events: Sequence[Event]) -> None: ...
    def send_paste(self) -> None:
        """Press Cmd+V."""
        ...

    def pasteboard_snapshot(self) -> PasteboardSnapshot: ...
    def pasteboard_set(self, text: str) -> None:
        """Set text, marked concealed and transient so clipboard managers do not keep it."""
        ...

    def pasteboard_clear(self) -> None: ...


@dataclass(frozen=True)
class MacInsertConfig:
    terminals: frozenset[str] = field(default_factory=lambda: frozenset(DEFAULT_TERMINALS))
    deny: frozenset[str] = field(default_factory=lambda: frozenset(DEFAULT_DENY))
    paste_delay: float = 0.4
    long_text: int = LONG_TEXT_CHARS
    notify: bool = True


def mac_insert_config_from(raw: Mapping[str, object] | None) -> MacInsertConfig:
    """From the `[insert]` table of dictate.toml. Names are bundle ids (`com.1password.1password`) or fragments."""
    raw = raw or {}
    kw: dict[str, object] = {}
    for key in ("terminals", "deny"):
        if key in raw:
            kw[key] = frozenset(str(v).casefold() for v in raw[key])  # type: ignore[attr-defined]
    if "paste_delay" in raw:
        kw["paste_delay"] = float(raw["paste_delay"])  # type: ignore[arg-type]
    if "notify" in raw:
        kw["notify"] = bool(raw["notify"])
    return MacInsertConfig(**kw)  # type: ignore[arg-type]


def plan_events(text: str) -> list[Event]:
    """UTF-16 code units (surrogate pairs kept together), with every newline form as one Return."""
    events: list[Event] = []
    i = 0
    while i < len(text):
        ch = text[i]
        if ch == "\r" and text[i + 1 : i + 2] == "\n":
            i += 1
            ch = "\n"
        if ch in "\r\n":
            events.append(("vk", KVK_RETURN))
        else:
            raw = ch.encode("utf-16-le", "surrogatepass")
            events.extend(("unit", int.from_bytes(raw[j : j + 2], "little")) for j in range(0, len(raw), 2))
        i += 1
    return events


def _units_to_text(units: list[int]) -> str:
    return b"".join(u.to_bytes(2, "little") for u in units).decode("utf-16-le", "surrogatepass")


def events_to_text(events: Sequence[Event]) -> str:
    """The inverse of `plan_events`: UTF-16 units back to text, Return back to a newline."""
    out: list[str] = []
    units: list[int] = []
    for kind, value in events:
        if kind == "unit":
            units.append(value)
            continue
        if units:
            out.append(_units_to_text(units))
            units = []
        out.append("\n")
    if units:
        out.append(_units_to_text(units))
    return "".join(out)


class _TypingStopped(Exception):
    """Typing failed after some text was already typed; `remainder` is the text that was not."""

    def __init__(self, remainder: str) -> None:
        super().__init__("typing stopped partway")
        self.remainder = remainder


def chunks(events: list[Event], size: int) -> list[list[Event]]:
    """Batches of at most `size` events that never split a surrogate pair."""
    out: list[list[Event]] = []
    cur: list[Event] = []
    for ev in events:
        if len(cur) >= size:
            carry: list[Event] = []
            if cur[-1][0] == "unit" and 0xD800 <= cur[-1][1] <= 0xDBFF:
                carry = [cur.pop()]  # the pair moves whole into the next batch, so no batch exceeds `size`
            out.append(cur)
            cur = carry
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


class MacInserter:
    def __init__(
        self,
        api: InsertApi,
        cfg: MacInsertConfig | None = None,
        *,
        notifier: Callable[[str], object] | None = None,
        sleep: Callable[[float], None] = time.sleep,
        schedule: Scheduler = _timer,
    ) -> None:
        self.api = api
        self.cfg = cfg or MacInsertConfig()
        self._notifier = notifier
        self._sleep = sleep
        self._schedule = schedule
        self._lock = threading.RLock()
        self._generation = 0
        self._pending_restore: PasteboardSnapshot | None = None  # the user's pasteboard, while ours is on it
        self._our_text: str | None = None
        self._ax_notified = False

    def notify(self, message: str) -> None:
        if self.cfg.notify and self._notifier is not None:
            self._notifier(message)

    # -- pasteboard -----------------------------------------------------------------------------
    def _retry(self, fn: Callable[[], object]) -> object:
        for attempt in range(PASTEBOARD_RETRIES):
            try:
                return fn()
            except PasteboardBusy:
                if attempt == PASTEBOARD_RETRIES - 1:
                    raise
                self._sleep(0.02 * (attempt + 1))
        raise AssertionError("unreachable")

    def _restore(self, generation: int) -> None:
        """Put the user's pasteboard back, unless a newer dictation owns it or the user copied something."""
        with self._lock:
            if generation != self._generation or self._pending_restore is None:
                return  # a later dictation took over; it restores the original instead
            saved, ours = self._pending_restore, self._our_text
            self._pending_restore = self._our_text = None
            try:
                now = self._retry(self.api.pasteboard_snapshot)
                if now.text != ours or now.other_formats:  # type: ignore[attr-defined]
                    return  # the user (or another app) changed it: leave theirs alone
                if saved.text:
                    self._retry(lambda: self.api.pasteboard_set(saved.text))  # type: ignore[arg-type]
                else:
                    self._retry(self.api.pasteboard_clear)
            except PasteboardBusy:
                log.warning("could not restore the pasteboard: it stayed busy")

    def _paste(self, text: str) -> bool:
        """Pasteboard + Cmd+V, restore later. False when it must not or cannot paste."""
        generation = -1
        with self._lock:
            try:
                snap = self._retry(self.api.pasteboard_snapshot)
                if snap.other_formats:  # type: ignore[attr-defined]
                    return False  # an image or files are on the pasteboard: do not destroy them
                self._generation += 1
                generation = self._generation
                if self._pending_restore is None:  # else the pasteboard holds our previous text, not the user's
                    self._pending_restore = snap  # type: ignore[assignment]
                self._our_text = text
                self.api.release_modifiers(0.3)  # wait first: keeps the window between the write and Cmd+V short
                self._retry(lambda: self.api.pasteboard_set(text))
                self.api.send_paste()
            except PasteboardBusy:
                self._pending_restore = self._our_text = None
                return False
            except Exception as exc:  # the paste keystroke was refused: put the pasteboard back, then type
                log.warning("paste failed: %s", type(exc).__name__)
                self._restore(generation)
                return False
        self._schedule(self.cfg.paste_delay, lambda: self._restore(generation))
        return True

    # -- typing ---------------------------------------------------------------------------------
    def _type(self, text: str) -> None:
        """Type `text`. On a failure after some batches posted, raise `_TypingStopped` with what is left."""
        self.api.release_modifiers(0.3)
        batches = chunks(plan_events(text), CHUNK_UNITS)
        for i, batch in enumerate(batches):
            try:
                self.api.send_events(batch)
            except Exception as exc:
                if i == 0:
                    raise
                raise _TypingStopped(events_to_text([ev for b in batches[i:] for ev in b])) from exc
            self._sleep(0.002)

    def _hand_over(self, text: str, why: str, *, notify: bool = True) -> InsertResult:
        with self._lock:
            try:
                self._retry(lambda: self.api.pasteboard_set(text))
            except PasteboardBusy as exc:  # the pending restore stays armed: the user's clipboard still comes back
                raise WordInkError("cannot insert text: the pasteboard is busy; the text is in `wordink-dictd last`") from exc
            self._generation += 1  # cancel any pending restore: this text must stay on the pasteboard
            self._pending_restore = self._our_text = None
        if notify:
            self.notify(why)
        return InsertResult("clipboard", why)

    # -- entry point ----------------------------------------------------------------------------
    def insert(self, text: str) -> InsertResult:
        if not text:
            return InsertResult("skipped", "empty")
        bundle = (self.api.frontmost_bundle_id() or "").casefold()
        if bundle and any(d in bundle for d in self.cfg.deny):
            self.notify("Skipped: a password manager is focused. Text is in `wordink-dictd last`.")
            return InsertResult("skipped", "denied app")
        if self.api.secure_input_enabled():
            return self._hand_over(text, "Copied to the pasteboard: a secure field has focus. Paste it with Cmd+V.")
        if not self.api.accessibility_trusted():
            first = not self._ax_notified  # explain how to fix it once, and only once the copy worked
            result = self._hand_over(
                text,
                "Copied to the pasteboard: grant Accessibility in System Settings, Privacy & Security, "
                "to let WordInk type. Paste it with Cmd+V.",
                notify=first,
            )
            self._ax_notified = True
            return result
        terminal = bundle in self.cfg.terminals
        if terminal or len(text) > self.cfg.long_text:
            if self._paste(text):
                return InsertResult("pasted", bundle or "long text")
        try:
            self._type(text)
        except _TypingStopped as stopped:  # the typed prefix is already in the app: hand over only the rest
            log.warning("typing stopped partway: %s", type(stopped.__cause__).__name__)
            return self._hand_over(
                stopped.remainder,
                "Typing stopped partway. The rest of the text is on the pasteboard: paste it with Cmd+V.",
            )
        except Exception as exc:
            log.warning("typing failed: %s", type(exc).__name__)
            return self._hand_over(text, "Copied to the pasteboard: typing failed. Paste it with Cmd+V.")
        return InsertResult("typed", "Quartz events")


class NativeInsertApi:
    """The real `InsertApi`: events and permissions from `quartz.py`, pasteboard and app from `appkit.py`.

    Imports are lazy and only here, so this module loads on every OS. Never run on a Mac before M9.
    """

    def __init__(self) -> None:
        from . import appkit, quartz  # noqa: PLC0415

        self._appkit, self._quartz = appkit, quartz

    def frontmost_bundle_id(self) -> str | None:
        return self._appkit.frontmost_bundle_id()

    def secure_input_enabled(self) -> bool:
        return self._quartz.secure_input_enabled()

    def accessibility_trusted(self) -> bool:
        return self._quartz.accessibility_trusted()

    def release_modifiers(self, wait: float) -> None:
        self._quartz.wait_modifiers_up(wait)

    def send_events(self, events: Sequence[Event]) -> None:
        self._quartz.post_events(events)

    def send_paste(self) -> None:
        self._quartz.post_paste()

    def pasteboard_snapshot(self) -> PasteboardSnapshot:
        text, other = self._appkit.pasteboard_snapshot()
        return PasteboardSnapshot(text, other)

    def pasteboard_set(self, text: str) -> None:
        if not self._appkit.pasteboard_set(text):
            raise PasteboardBusy("AppKit refused the pasteboard write")

    def pasteboard_clear(self) -> None:
        self._appkit.pasteboard_clear()
