"""System tray indicator and menu for Windows (plan W6).

`pystray` and Pillow are imported lazily and can be injected, so state mapping, menu wiring and
toast behavior are tested on Linux with a fake. Icons are drawn as raw RGBA bytes in pure Python
and only converted to a Pillow image at the last moment.
"""

from __future__ import annotations

import logging
from collections.abc import Callable
from dataclasses import dataclass
from types import ModuleType

from ...client import WordInkError

log = logging.getLogger("wordink.dictd")

ICON_SIZE = 64

#: state -> (RGB of the dot, tooltip text)
STATES: dict[str, tuple[tuple[int, int, int], str]] = {
    "idle": ((120, 130, 140), "WordInk: ready"),
    "recording": ((220, 38, 38), "WordInk: listening"),
    "transcribing": ((245, 158, 11), "WordInk: transcribing"),
    "error": ((127, 29, 29), "WordInk: error"),
    "off": ((60, 60, 60), "WordInk: stopped"),
}
MAX_TOOLTIP = 120  # Windows truncates notification-area tooltips at 127 characters


def render_icon(state: str, size: int = ICON_SIZE) -> bytes:
    """RGBA bytes: a filled circle in the state's color on a transparent square, with a white
    bar through it for `error` so the state does not rely on color alone."""
    r, g, b = STATES.get(state, STATES["off"])[0]
    c, radius = (size - 1) / 2, size * 0.42
    out = bytearray()
    for y in range(size):
        for x in range(size):
            inside = (x - c) ** 2 + (y - c) ** 2 <= radius**2
            mark = state == "error" and abs(x - c) <= size * 0.06 and size * 0.28 <= y <= size * 0.72
            if inside and mark:
                out += bytes((255, 255, 255, 255))
            elif inside:
                out += bytes((r, g, b, 255))
            else:
                out += bytes((0, 0, 0, 0))
    return bytes(out)


def tooltip(state: str, detail: str | None = None) -> str:
    text = STATES.get(state, STATES["off"])[1]
    if detail and state == "error":
        text = f"{text}: {detail}"
    return text if len(text) <= MAX_TOOLTIP else text[: MAX_TOOLTIP - 1] + "…"


@dataclass(frozen=True)
class TrayActions:
    """What the menu does. The app wires these to the in-process daemon and the OS."""

    toggle: Callable[[], object]
    copy_last: Callable[[], object]
    open_config: Callable[[], object]
    open_log: Callable[[], object]
    quit: Callable[[], object]


def _load(name: str) -> ModuleType:
    try:
        return __import__(name)
    except ImportError as exc:
        raise WordInkError(f"the tray needs the '{name}' package: install wordink with the 'windows' extra") from exc


def _pil_image(rgba: bytes, size: int, pil: ModuleType | None = None):
    image_mod = pil or __import__("PIL.Image", fromlist=["Image"])
    return image_mod.frombytes("RGBA", (size, size), rgba)


class Tray:
    """An `Indicator`: `show(state, detail)` repaints the icon and tooltip; menu items call `actions`."""

    def __init__(
        self,
        actions: TrayActions,
        *,
        pystray: ModuleType | None = None,
        make_image: Callable[[bytes, int], object] | None = None,
    ) -> None:
        self.actions = actions
        self._pystray = pystray
        self._make_image = make_image or _pil_image
        self._icon = None
        self.state = "off"
        self._error_toasted = False

    # -- menu -----------------------------------------------------------------------------------
    def _menu(self, ps: ModuleType):
        item = ps.MenuItem
        return ps.Menu(
            item("Start / stop dictation", lambda *_: self.actions.toggle()),
            item("Copy last transcript", lambda *_: self.actions.copy_last()),
            ps.Menu.SEPARATOR,
            item("Open config folder", lambda *_: self.actions.open_config()),
            item("Open log", lambda *_: self.actions.open_log()),
            ps.Menu.SEPARATOR,
            item("Quit WordInk", lambda *_: self._quit()),
        )

    def _quit(self) -> None:
        self.actions.quit()
        self.stop()

    # -- lifecycle ------------------------------------------------------------------------------
    def _image(self, state: str):
        return self._make_image(render_icon(state), ICON_SIZE)

    def start(self) -> None:
        """Show the icon on its own thread (returns at once)."""
        if self._icon is not None:
            return
        ps = self._pystray or _load("pystray")
        self._icon = ps.Icon("wordink", self._image("off"), tooltip("off"), self._menu(ps))
        self._icon.run_detached()

    def stop(self) -> None:
        icon, self._icon = self._icon, None
        if icon is not None:
            icon.stop()

    # -- Indicator ------------------------------------------------------------------------------
    def show(self, state: str, detail: str | None = None) -> None:
        if state not in STATES:
            state = "off"
        previous, self.state = self.state, state
        if state != "error":
            self._error_toasted = False
        icon = self._icon
        if icon is None:
            return
        try:
            if state != previous:
                icon.icon = self._image(state)
            icon.title = tooltip(state, detail)
            if state == "error" and not self._error_toasted:
                self._error_toasted = True
                icon.notify(detail or "Dictation failed", "WordInk")
        except Exception as exc:  # a tray glitch must never take dictation down
            log.warning("tray update failed: %s", type(exc).__name__)
