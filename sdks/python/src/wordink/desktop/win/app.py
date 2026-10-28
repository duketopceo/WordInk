"""The Windows dictation app: wires the adapters to the shared daemon and runs it (plan W7b).

Everything OS-specific is passed in through `Adapters`, so the whole app runs on Linux in tests
with fakes. `main()` builds the real adapters lazily and is the `wordink-dictd-tray` entry point.
"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import logging.handlers
import os
import sys
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
from .hook import HookUnavailable
from .insert import WindowsInserter, win_insert_config_from
from .secrets import KeyStore, env_with_stored_key
from .tray import Tray, TrayActions

log = logging.getLogger("wordink.dictd")


@dataclass
class Adapters:
    audio: AudioSource
    inserter: Any  # anything with insert(text) -> InsertResult
    hotkeys: HotkeySource | None
    transport: Transport
    tray: Tray | None = None
    clipboard_set: Callable[[str], None] | None = None  # for the "copy last transcript" menu item
    open_path: Callable[[Path], None] | None = None  # os.startfile on Windows
    fallback_hotkeys: HotkeySource | None = None  # toggle-only RegisterHotKey when the hook is blocked


class _StateAndTray(StateFile):
    """The state file the CLI reads, mirrored to the tray icon."""

    def __init__(self, path: Path, tray: Tray | None) -> None:
        super().__init__(path)
        self._tray = tray

    def write(self, state: str, detail: str | None = None) -> None:
        super().write(state, detail)
        if self._tray is not None:
            self._tray.show(state, detail)


class WinApp:
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
        self._active_source: HotkeySource | None = None
        last = LastTranscriptSink(cfg.last_path)

        def sink(text: str) -> None:
            # Deliberately BEFORE inserting: if typing or pasting fails, the transcript is still in
            # last.txt (and the tray's "Copy last transcript"), so a dictation is never lost.
            last(text)
            res = adapters.inserter.insert(text)
            log.info("inserted via %s%s", res.method, f" ({res.detail})" if res.detail else "")

        self.daemon = Daemon(
            cfg,
            build_pipeline(cfg, env, transport=http),
            sink,
            recorder=Recorder(min_seconds=cfg.min_seconds, tail_seconds=cfg.tail_seconds, source=adapters.audio),
            indicator=_StateAndTray(cfg.state_path, adapters.tray),
            transport=adapters.transport,
        )
        if adapters.tray is not None:
            adapters.tray.actions = TrayActions(
                toggle=lambda: self._command("toggle"),
                copy_last=self._copy_last,
                open_config=lambda: self._open(cfg.vocab_path.parent),
                open_log=lambda: self._open(log_path(cfg)),
                quit=self._request_stop,
            )

    # -- thread-safe entry points (hotkeys and the tray run on their own threads) ---------------
    def _command(self, name: str) -> None:
        loop = self._loop
        if loop is not None and loop.is_running():
            asyncio.run_coroutine_threadsafe(self.daemon.handle(name), loop)

    def _request_stop(self) -> None:
        loop = self._loop
        if loop is not None:
            loop.call_soon_threadsafe(self.stop_event.set)

    def _copy_last(self) -> None:
        if self.ad.clipboard_set is None:
            return
        try:
            self.ad.clipboard_set(self.cfg.last_path.read_text(encoding="utf-8"))
        except OSError:
            log.info("no last transcript to copy yet")

    def _open(self, path: Path) -> None:
        if self.ad.open_path is not None:
            self.ad.open_path(path)

    # -- lifecycle ------------------------------------------------------------------------------
    async def start(self) -> None:
        self._loop = asyncio.get_running_loop()
        await self.daemon.serve()  # raises WordInkError("already running") for a second instance
        if self.ad.tray is not None:
            self.ad.tray.start()
            self.ad.tray.show("idle")
        if self.ad.hotkeys is not None:
            try:
                self.ad.hotkeys.start(
                    lambda: self._command("start"),
                    lambda: self._command("stop"),
                    lambda: self._command("toggle"),
                )
                self.hotkeys_active = True
                self._active_source = self.ad.hotkeys
            except HookUnavailable as exc:
                log.error("%s", exc)
                self._start_fallback()

    def _start_fallback(self) -> None:
        """The hook is blocked: toggle-only hotkeys if they can be registered, else the tray menu only."""
        tray = self.ad.tray
        fb = self.ad.fallback_hotkeys
        if fb is not None:
            try:
                fb.start(lambda: None, lambda: None, lambda: self._command("toggle"))
            except WordInkError as exc:
                log.error("fallback hotkeys failed: %s", exc)
            else:
                self.hotkeys_active, self._active_source = True, fb
                if tray is not None:
                    tray.show("idle")
                    tray.notice("Hold-to-talk is blocked on this PC; the hotkey now toggles recording (press to start, press to stop).")
                return
        if tray is not None:
            tray.show("error", "hotkey blocked: use the tray menu to dictate")

    async def close(self) -> None:
        if self._active_source is not None and self.hotkeys_active:
            await asyncio.to_thread(self._active_source.stop)
        await self.daemon.close()
        if self.ad.tray is not None:
            self.ad.tray.stop()


def log_path(cfg: DictateConfig) -> Path:
    return cfg.runtime_dir.parent / "dictd.log"


def setup_logging(cfg: DictateConfig, debug: bool = False) -> None:
    path = log_path(cfg)
    path.parent.mkdir(parents=True, exist_ok=True)
    handler = logging.handlers.RotatingFileHandler(path, maxBytes=1_000_000, backupCount=3, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.addHandler(handler)
    root.setLevel(logging.DEBUG if debug else logging.INFO)
    for noisy in ("httpx", "httpcore", "hpack"):  # their debug lines can carry request headers
        logging.getLogger(noisy).setLevel(logging.WARNING)


def real_adapters(cfg: DictateConfig) -> Adapters:
    """The Windows implementations. Imported lazily: only the tray app on Windows gets here."""
    from .audio import SoundDeviceSource
    from .hook import HookHotkeySource
    from .hotkey import parse_chords
    from .transport import LoopbackTransport
    from .win32 import RegisterHotKeySource, Win32, Win32Hook

    api = Win32()
    hold, toggle = parse_chords(cfg.hold_bind, cfg.toggle_bind)
    return Adapters(
        audio=SoundDeviceSource(),
        inserter=WindowsInserter(api, win_insert_config_from(cfg.insert)),
        hotkeys=HookHotkeySource(hold, toggle, Win32Hook()),
        transport=LoopbackTransport(cfg.runtime_dir),
        tray=Tray(),
        fallback_hotkeys=RegisterHotKeySource(hold, toggle),
        clipboard_set=api.clipboard_set,
        open_path=os.startfile if hasattr(os, "startfile") else None,  # type: ignore[attr-defined]
    )


async def run_app(cfg: DictateConfig, adapters: Adapters, env: Mapping[str, str], *, http: httpx.BaseTransport | None = None) -> int:
    app = WinApp(cfg, adapters, env, http=http)
    await app.start()
    log.info("wordink-dictd (Windows) is running; hold %s to dictate", cfg.hold_bind)
    try:
        await app.stop_event.wait()
    finally:
        await app.close()
    return 0


def run_configured(cfg: DictateConfig, debug: bool = False) -> int:
    """Log to a file, take the key from Credential Manager, and run the tray app until Quit."""
    setup_logging(cfg, debug)
    env = env_with_stored_key(os.environ, KeyStore())
    return asyncio.run(run_app(cfg, real_adapters(cfg), env))


def main(argv: list[str] | None = None) -> int:
    """`wordink-dictd-tray`: run the dictation app with a tray icon. No console window."""
    try:
        return run_configured(load_config(os_name="windows"), os.environ.get("WORDINK_DEBUG") == "1")
    except WordInkError as exc:
        logging.getLogger("wordink.dictd").error("%s", exc)
        with contextlib.suppress(Exception):
            print(f"wordink-dictd: {exc}", file=sys.stderr)
        return 1
