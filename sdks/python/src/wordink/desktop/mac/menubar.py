"""Menu-bar status item and notifications for macOS (plan M5).

AppKit must be touched on the main thread, and the daemon runs on a worker thread. So the daemon
side (`show`, `notice`) only puts messages on a thread-safe queue, and a `rumps.Timer` on the main
thread drains it and updates the status item. `rumps` is imported lazily and can be injected, so
the whole mapping and wiring is tested on Linux with a fake. Not run on a Mac yet (plan M9).
"""

from __future__ import annotations

import logging
import queue
from collections.abc import Callable
from dataclasses import dataclass
from types import ModuleType

from ...client import WordInkError

log = logging.getLogger("wordink.dictd")

#: state -> (menu-bar title, tooltip / status line). Symbols differ by shape, not only color.
STATES: dict[str, tuple[str, str]] = {
    "idle": ("🎙", "WordInk: ready"),
    "recording": ("🔴", "WordInk: listening"),
    "transcribing": ("⏳", "WordInk: transcribing"),
    "error": ("⚠️", "WordInk: error"),
    "off": ("⏸", "WordInk: stopped"),
}
MAX_TOOLTIP = 120
DRAIN_INTERVAL = 0.2


def tooltip(state: str, detail: str | None = None) -> str:
    text = STATES.get(state, STATES["off"])[1]
    if detail and state == "error":
        text = f"{text}: {detail}"
    return text if len(text) <= MAX_TOOLTIP else text[: MAX_TOOLTIP - 1] + "…"


@dataclass(frozen=True)
class MenuActions:
    """What the menu does. The app wires these to a thread-safe handle on the daemon and the OS."""

    toggle: Callable[[], object]
    copy_last: Callable[[], object]
    open_config: Callable[[], object]
    open_log: Callable[[], object]
    quit: Callable[[], object]


def _load_rumps() -> ModuleType:
    try:
        import rumps  # noqa: PLC0415
    except ImportError as exc:
        raise WordInkError("the menu bar needs the 'rumps' package: install wordink with the 'macos' extra") from exc
    return rumps


class MenuBar:
    """An `Indicator`. `show` and `notice` are safe from any thread; `run` blocks on the main thread."""

    def __init__(self, actions: MenuActions | None = None, *, rumps: ModuleType | None = None) -> None:
        self.actions = actions or MenuActions(*(lambda: None,) * 5)  # the app installs the real ones
        self._rumps = rumps
        self._queue: queue.SimpleQueue[tuple[str, str, str | None]] = queue.SimpleQueue()
        self.state = "off"
        self._last_error: str | None = None
        self._app = None
        self._status_item = None
        self.on_tick: Callable[[], None] | None = None  # runs on the main thread, once per drain

    # -- daemon side (any thread): only enqueue -----------------------------------------------------
    def show(self, state: str, detail: str | None = None) -> None:
        self._queue.put(("state", state if state in STATES else "off", detail))

    def notice(self, message: str) -> None:
        """A one-off notification that is not an error (for example, a degraded mode)."""
        self._queue.put(("notice", message, None))

    # -- main thread ----------------------------------------------------------------------------------
    def _menu_items(self, rumps: ModuleType) -> list:
        self._status_item = rumps.MenuItem(tooltip("off"))
        self._status_item.set_callback(None)  # a label, not a button
        return [
            self._status_item,
            None,
            rumps.MenuItem("Start / stop dictation", callback=lambda _: self.actions.toggle()),
            rumps.MenuItem("Copy last transcript", callback=lambda _: self.actions.copy_last()),
            None,
            rumps.MenuItem("Open config folder", callback=lambda _: self.actions.open_config()),
            rumps.MenuItem("Open log", callback=lambda _: self.actions.open_log()),
            None,
            rumps.MenuItem("Quit WordInk", callback=lambda _: self._quit()),
        ]

    def _quit(self) -> None:
        try:
            self.actions.quit()
        finally:
            (self._rumps or _load_rumps()).quit_application()

    def drain(self, _timer: object = None) -> None:
        """Apply queued updates. Called by the main-thread timer; tests call it directly."""
        rumps = self._rumps or _load_rumps()
        self._run_tick()
        while True:
            try:
                kind, a, b = self._queue.get_nowait()
            except queue.Empty:
                return
            try:
                if kind == "state":
                    self._apply_state(rumps, a, b)
                else:
                    rumps.notification("WordInk", "", a[:300])
            except Exception as exc:  # a menu-bar glitch must never take dictation down
                log.warning("menu bar update failed: %s", type(exc).__name__)

    def _run_tick(self) -> None:
        if self.on_tick is None:
            return
        try:
            self.on_tick()
        except Exception as exc:  # a failing hook must never take the menu bar down
            log.warning("menu bar tick failed: %s", type(exc).__name__)

    def _apply_state(self, rumps: ModuleType, state: str, detail: str | None) -> None:
        self.state = state
        if self._app is not None:
            self._app.title = STATES[state][0]
        if self._status_item is not None:
            self._status_item.title = tooltip(state, detail)
        if state != "error":
            self._last_error = None
            return
        message = detail or "Dictation failed"
        if message != self._last_error:  # once per distinct failure
            self._last_error = message
            rumps.notification("WordInk", "", message[:300])

    def run(self) -> None:
        """Show the status item and run the Cocoa run loop. Blocks; call on the main thread."""
        rumps = self._rumps or _load_rumps()
        self._app = rumps.App("WordInk", title=STATES["off"][0], menu=self._menu_items(rumps), quit_button=None)
        rumps.Timer(self.drain, DRAIN_INTERVAL).start()
        self._app.run()

    def stop(self) -> None:
        """Ask the run loop to end; safe from any thread (rumps routes it to the main loop)."""
        (self._rumps or _load_rumps()).quit_application()
