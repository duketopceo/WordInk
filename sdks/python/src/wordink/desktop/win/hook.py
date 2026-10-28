"""The global keyboard hook as a `HotkeySource` (plan W4b).

The OS-facing half (`HookApi`) is a protocol; `win32.py` implements it with `WH_KEYBOARD_LL`. This
module owns the threads and the safety rules:

* the hook callback only classifies the event and queues it, so it returns in microseconds;
* a dispatcher thread, not the hook thread, calls the daemon;
* a watchdog injects a harmless heartbeat key and reinstalls the hook when it does not arrive,
  because Windows removes a hook whose callback was ever too slow, without telling anyone;
* if the hook cannot be installed at all, `start()` raises `HookUnavailable` so the app can fall
  back to a toggle-only `RegisterHotKey` mode.
"""

from __future__ import annotations

import logging
import queue
import threading
import time
from collections.abc import Callable
from typing import Protocol

from ...client import WordInkError
from .hotkey import HOLD_PRESS, HOLD_RELEASE, TOGGLE, Chord, ChordTracker

log = logging.getLogger("wordink.dictd")

#: VK_NONAME: a reserved, unassigned code. The watchdog injects it; nothing reacts to it.
HEARTBEAT_VK = 0xFC

KeyCallback = Callable[[int, bool, bool], bool]  # (vk, is_down, injected) -> swallow


class HookUnavailable(WordInkError):
    """The low-level keyboard hook could not be installed (blocked by policy or security software)."""


class HookApi(Protocol):
    def run_hook(self, on_key: KeyCallback, ready: Callable[[], None]) -> None:
        """Install the hook on this thread, call `ready()`, then pump messages until `stop_hook()`.
        Raises OSError if the hook cannot be installed."""
        ...

    def stop_hook(self) -> None:
        """Make `run_hook` return (post WM_QUIT to the hook thread). Safe when no hook runs."""
        ...

    def send_heartbeat(self) -> None:
        """Inject a key-up of `HEARTBEAT_VK`, which a live hook sees with `injected=True`."""
        ...

    def probe_hotkey(self, chord: Chord) -> bool:
        """Try `RegisterHotKey` for the chord and unregister at once. False means another app owns it."""
        ...


def probe_clashes(api: HookApi, *chords: Chord) -> list[str]:
    """The chords another application already registered (they would not reach our hook reliably)."""
    return [c.text for c in chords if not api.probe_hotkey(c)]


class HookHotkeySource:
    def __init__(
        self,
        hold: Chord,
        toggle: Chord,
        api: HookApi,
        *,
        heartbeat_interval: float = 5.0,
        heartbeat_timeout: float = 0.5,
        install_timeout: float = 3.0,
    ) -> None:
        self.api = api
        self.tracker = ChordTracker(hold, toggle)
        self.heartbeat_interval = heartbeat_interval
        self.heartbeat_timeout = heartbeat_timeout
        self.install_timeout = install_timeout
        self.reinstalls = 0
        self._lock = threading.Lock()
        self._queue: queue.SimpleQueue[str | None] = queue.SimpleQueue()
        self._stop = threading.Event()
        self._beat = 0.0
        self._handlers: dict[str, Callable[[], None]] = {}
        self._hook_thread: threading.Thread | None = None
        self._threads: list[threading.Thread] = []
        self._install_error: Exception | None = None

    # -- the hook callback: must stay tiny --------------------------------------------------------
    def _on_key(self, vk: int, down: bool, injected: bool) -> bool:
        if vk == HEARTBEAT_VK:
            self._beat = time.monotonic()
            return False
        with self._lock:
            result = self.tracker.feed(vk, down, injected)
        for event in result.events:
            self._queue.put_nowait(event)
        return result.swallow

    # -- threads ----------------------------------------------------------------------------------
    def _hook_main(self, ready: threading.Event) -> None:
        try:
            self.api.run_hook(self._on_key, ready.set)
        except Exception as exc:
            self._install_error = exc
            log.error("keyboard hook stopped: %s", type(exc).__name__)
        finally:
            ready.set()  # never leave start() waiting

    def _spawn_hook(self) -> None:
        ready = threading.Event()
        self._install_error = None
        thread = threading.Thread(target=self._hook_main, args=(ready,), name="wordink-hook", daemon=True)
        self._hook_thread = thread
        thread.start()
        if not ready.wait(self.install_timeout) or self._install_error is not None:
            reason = type(self._install_error).__name__ if self._install_error else "timed out"
            raise HookUnavailable(
                f"cannot install the keyboard hook ({reason}); security software may be blocking it, "
                "so dictation falls back to toggle-only mode"
            )

    def _dispatch(self) -> None:
        while (event := self._queue.get()) is not None:
            handler = self._handlers.get(event)
            if handler is None:
                continue
            try:
                handler()
            except Exception:  # a daemon error must not kill the dispatcher
                log.exception("hotkey handler failed")

    def _heartbeat_ok(self) -> bool:
        before = self._beat
        self.api.send_heartbeat()
        deadline = time.monotonic() + self.heartbeat_timeout
        while time.monotonic() < deadline:
            if self._beat != before:
                return True
            time.sleep(0.005)
        return self._beat != before

    def _watchdog(self) -> None:
        while not self._stop.wait(self.heartbeat_interval):
            try:
                if self._heartbeat_ok():
                    continue
                log.warning("keyboard hook went quiet; reinstalling")
                self._reinstall()
            except Exception as exc:
                log.error("hook watchdog: %s", type(exc).__name__)

    def _reinstall(self) -> None:
        self.api.stop_hook()
        if self._hook_thread is not None:
            self._hook_thread.join(2.0)
        with self._lock:
            was_holding = self.tracker.holding
            self.tracker.reset()  # keys may have changed while the hook was dead
        if was_holding:
            self._queue.put_nowait(HOLD_RELEASE)  # never leave the microphone recording
        self.reinstalls += 1
        self._spawn_hook()

    # -- HotkeySource -----------------------------------------------------------------------------
    def start(
        self,
        on_hold_press: Callable[[], None],
        on_hold_release: Callable[[], None],
        on_toggle: Callable[[], None],
    ) -> None:
        if self._hook_thread is not None:
            raise WordInkError("the hotkey source is already running")
        self._handlers = {HOLD_PRESS: on_hold_press, HOLD_RELEASE: on_hold_release, TOGGLE: on_toggle}
        self._stop.clear()
        self._spawn_hook()  # raises HookUnavailable before any helper thread exists
        for target, name in ((self._dispatch, "wordink-hotkeys"), (self._watchdog, "wordink-hook-watchdog")):
            t = threading.Thread(target=target, name=name, daemon=True)
            self._threads.append(t)
            t.start()

    def stop(self) -> None:
        self._stop.set()
        self.api.stop_hook()
        self._queue.put_nowait(None)
        for t in [self._hook_thread, *self._threads]:
            if t is not None:
                t.join(2.0)
        self._hook_thread = None
        self._threads = []
