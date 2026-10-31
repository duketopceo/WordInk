"""The macOS dictation app: wires the adapters to the shared daemon and runs it (plan M6).

The Cocoa run loop must own the main thread, so the menu bar runs there and the daemon's asyncio
loop runs on a worker thread. The hotkeys are registered on the main thread too (macOS kills the
app for Carbon calls from another thread) and that same run loop delivers their events.
Everything OS-specific comes in through `Adapters`, so the whole app runs on Linux in tests with
fakes. `main()` builds the real adapters lazily; it is the `wordink-dictd-mac` entry point and
what the LaunchAgent starts (`wordink-dictd run` does the same).
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import logging.handlers
import os
import subprocess
import sys
import threading
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import httpx

from ...client import WordInkError
from ..config import DictateConfig, load_config
from ..daemon import Daemon, LastTranscriptSink, build_pipeline
from ..indicator import StateFile
from ..interfaces import AudioSource, HotkeySource, Transport
from ..recorder import Recorder
from .insert import MacInserter, mac_insert_config_from
from .menubar import MenuActions, MenuBar
from .secrets import KeyStore, env_with_stored_key

log = logging.getLogger("wordink.dictd")


@dataclass
class Adapters:
    audio: AudioSource
    inserter: Any  # anything with insert(text) -> InsertResult
    hotkeys: HotkeySource | None
    menubar: MenuBar | None = None
    transport: Transport | None = None  # None: the Unix socket under cfg.runtime_dir
    clipboard_set: Callable[[str], None] | None = None  # for "Copy last transcript"
    open_path: Callable[[Path], None] | None = None  # `open` on a Mac


class _StateAndMenu(StateFile):
    """The state file the CLI reads, mirrored to the menu bar."""

    def __init__(self, path: Path, menubar: MenuBar | None) -> None:
        super().__init__(path)
        self._menubar = menubar

    def write(self, state: str, detail: str | None = None) -> None:
        super().write(state, detail)
        if self._menubar is not None:
            self._menubar.show(state, detail)


class MacApp:
    def __init__(
        self,
        cfg: DictateConfig,
        adapters: Adapters,
        env: Mapping[str, str],
        *,
        http: httpx.BaseTransport | None = None,
    ) -> None:
        self.cfg, self.ad = cfg, adapters
        self.stop_event = asyncio.Event()
        self._loop: asyncio.AbstractEventLoop | None = None
        self.hotkeys_active = False
        self._hotkeys_tried = False
        last = LastTranscriptSink(cfg.last_path)

        def sink(text: str) -> None:
            # Deliberately BEFORE inserting: if typing or pasting fails, the transcript is still in
            # last.txt (and the menu's "Copy last transcript"), so a dictation is never lost.
            last(text)
            res = adapters.inserter.insert(text)
            log.info("inserted via %s%s", res.method, f" ({res.detail})" if res.detail else "")

        self.daemon = Daemon(
            cfg,
            build_pipeline(cfg, env, transport=http),
            sink,
            recorder=Recorder(min_seconds=cfg.min_seconds, tail_seconds=cfg.tail_seconds, source=adapters.audio),
            indicator=_StateAndMenu(cfg.state_path, adapters.menubar),
            transport=adapters.transport,
        )
        if adapters.menubar is not None:
            adapters.menubar.actions = MenuActions(
                toggle=lambda: self._command("toggle"),
                copy_last=self._copy_last,
                open_config=lambda: self._open(cfg.vocab_path.parent),
                open_log=lambda: self._open(log_path()),
                quit=self.request_stop,
            )
            adapters.menubar.on_tick = self._main_thread_tick

    # -- thread-safe entry points (hotkey events and the menu arrive on the main thread) -------
    def _command(self, name: str) -> None:
        loop = self._loop
        if loop is not None and loop.is_running():
            asyncio.run_coroutine_threadsafe(self.daemon.handle(name), loop)

    def request_stop(self) -> None:
        loop = self._loop
        if loop is not None:
            loop.call_soon_threadsafe(self.stop_event.set)

    def _copy_last(self) -> None:
        if self.ad.clipboard_set is None:
            return
        try:
            self.ad.clipboard_set(self.cfg.last_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):  # no transcript yet, or an unreadable file
            log.info("no last transcript to copy yet")

    def _open(self, path: Path) -> None:
        if self.ad.open_path is not None:
            self.ad.open_path(path)

    # -- lifecycle ------------------------------------------------------------------------------
    async def start(self) -> None:
        self._loop = asyncio.get_running_loop()
        await self.daemon.serve()  # raises WordInkError("already running") for a second instance
        menubar = self.ad.menubar
        if menubar is not None:
            menubar.show("idle")

    # -- hotkeys: main thread only ------------------------------------------------------------------
    def start_hotkeys(self) -> None:
        """Register the chords. Main thread only, once, inside the running Cocoa loop."""
        self._hotkeys_tried = True
        hotkeys, menubar = self.ad.hotkeys, self.ad.menubar
        if hotkeys is None:
            return
        try:
            hotkeys.start(
                lambda: self._command("start"),
                lambda: self._command("stop"),
                lambda: self._command("toggle"),
            )
        except WordInkError as exc:  # a clash: dictation still works from the menu and `trigger`
            log.error("%s", exc)
            if menubar is not None:
                menubar.show("error", "hotkey unavailable: use the menu or `wordink-dictd trigger toggle`")
                menubar.notice(str(exc))
        else:
            self.hotkeys_active = True

    def stop_hotkeys(self) -> None:
        """Unregister the chords. Main thread only; `run_with_menu` calls it after the loop ends."""
        if self.hotkeys_active and self.ad.hotkeys is not None:
            self.ad.hotkeys.stop()
            self.hotkeys_active = False

    def _main_thread_tick(self) -> None:
        """Run by the menu bar's main-thread timer: register on the first tick, then watch for wake."""
        if not self._hotkeys_tried:
            self.start_hotkeys()
        elif self.hotkeys_active:
            poll = getattr(self.ad.hotkeys, "poll", None)
            if poll is not None:
                poll()

    async def close(self) -> None:
        await self.daemon.close()


def log_path(env: Mapping[str, str] | None = None) -> Path:
    home = (os.environ if env is None else env).get("HOME")
    if not home:
        raise WordInkError("cannot find the home folder: HOME is unset")
    return Path(home) / "Library" / "Logs" / "wordink" / "dictd.log"


def setup_logging(debug: bool = False, env: Mapping[str, str] | None = None) -> None:
    path = log_path(env)
    path.parent.mkdir(parents=True, exist_ok=True)
    handler = logging.handlers.RotatingFileHandler(path, maxBytes=1_000_000, backupCount=3, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.addHandler(handler)
    root.setLevel(logging.DEBUG if debug else logging.INFO)
    for noisy in ("httpx", "httpcore", "hpack"):  # their debug lines can carry request headers
        logging.getLogger(noisy).setLevel(logging.WARNING)


def _open_with_finder(path: Path) -> None:
    done = subprocess.run(["open", str(path)], check=False, timeout=10, capture_output=True)  # noqa: S603, S607
    if done.returncode != 0:
        log.warning("`open` could not open %s (exit %s)", path.name, done.returncode)


def real_adapters(cfg: DictateConfig) -> Adapters:
    """The macOS implementations. Imported lazily: only the Mac app gets here."""
    from . import appkit
    from .audio import SoundDeviceSource
    from .hotkey import HotkeySource as CarbonHotkeys
    from .hotkey import parse_chords
    from .insert import NativeInsertApi
    from .quartz import CarbonApi

    menubar = MenuBar()
    hold, toggle = parse_chords(cfg.hold_bind, cfg.toggle_bind)
    return Adapters(
        audio=SoundDeviceSource(),
        inserter=MacInserter(NativeInsertApi(), mac_insert_config_from(cfg.insert), notifier=menubar.notice),
        hotkeys=CarbonHotkeys(hold, toggle, CarbonApi()),
        menubar=menubar,
        clipboard_set=appkit.pasteboard_copy,
        open_path=_open_with_finder,
    )


async def run_app(
    cfg: DictateConfig,
    adapters: Adapters,
    env: Mapping[str, str],
    *,
    http: httpx.BaseTransport | None = None,
    started: Callable[[MacApp | None, BaseException | None], None] | None = None,
) -> int:
    """Start, wait for Quit, close. `started` reports the app (or the startup error) to another thread."""
    app = MacApp(cfg, adapters, env, http=http)
    try:
        await app.start()
    except BaseException as exc:
        if started is not None:
            started(None, exc)
        await app.close()
        raise
    if started is not None:
        started(app, None)
    log.info("wordink-dictd (macOS) is running; hold %s to dictate", cfg.hold_bind)
    try:
        await app.stop_event.wait()
    finally:
        await app.close()
    return 0


def run_with_menu(
    cfg: DictateConfig,
    adapters: Adapters,
    env: Mapping[str, str],
    *,
    http: httpx.BaseTransport | None = None,
    startup_timeout: float = 15.0,
) -> int:
    """Daemon on a worker thread, menu bar on the calling (main) thread. Returns when Quit is chosen."""
    menubar = adapters.menubar
    ready = threading.Event()
    result: dict[str, Any] = {"error": None, "code": 0, "app": None}

    def started(app: MacApp | None, exc: BaseException | None) -> None:
        result["error"], result["app"] = exc, app
        ready.set()

    def worker() -> None:
        try:
            result["code"] = asyncio.run(run_app(cfg, adapters, env, http=http, started=started))
        except BaseException as exc:  # reported below, on the main thread
            result["error"] = result["error"] or exc
        finally:
            ready.set()
            if menubar is not None:
                with contextlib.suppress(Exception):
                    menubar.stop()  # the daemon is gone: end the run loop too

    thread = threading.Thread(target=worker, name="wordink-daemon", daemon=True)
    thread.start()
    if not ready.wait(startup_timeout):
        raise WordInkError("the daemon did not start in time")
    if result["error"] is not None:
        thread.join(5.0)
        err = result["error"]
        raise err if isinstance(err, WordInkError) else WordInkError(f"cannot start: {type(err).__name__}")
    app: MacApp = result["app"]
    try:
        if menubar is not None:
            menubar.run()  # blocks until Quit (or the worker ends); its timer registers the hotkeys
        else:
            app.start_hotkeys()
    except BaseException:
        with contextlib.suppress(RuntimeError):  # the loop may already be closed
            app.request_stop()
        raise
    finally:
        app.stop_hotkeys()
    thread.join(10.0)
    err = result["error"]
    if err is not None:
        raise err if isinstance(err, WordInkError) else WordInkError(f"stopped: {type(err).__name__}")
    return int(result["code"])


def run_configured(cfg: DictateConfig, debug: bool = False) -> int:
    """Log to a file, take the key from the Keychain, and run the menu-bar app until Quit."""
    setup_logging(debug)
    env = env_with_stored_key(os.environ, KeyStore())
    return run_with_menu(cfg, real_adapters(cfg), env)


def main(argv: list[str] | None = None) -> int:
    """`wordink-dictd-mac`: run the dictation app with a menu-bar icon."""
    try:
        return run_configured(load_config(os_name="macos"), os.environ.get("WORDINK_DEBUG") == "1")
    except WordInkError as exc:
        logging.getLogger("wordink.dictd").error("%s", exc)
        with contextlib.suppress(Exception):
            print(f"wordink-dictd: {exc}", file=sys.stderr)
        return 1
