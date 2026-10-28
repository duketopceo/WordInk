"""wordink-dictd: a Unix-socket daemon that turns a Hyprland bind into recorded, transcribed, cleaned text."""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import os
import socket
import tempfile
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

import httpx

from ..client import WordInkError
from .cleanup import CleanupConfig, clean_transcript
from .config import DictateConfig
from .indicator import StateFile
from .recorder import Recorder
from .transcribe import TranscriptionError, resolve_transcribe_config, transcribe_audio
from .vocab import load_vocabulary

log = logging.getLogger("wordink.dictd")

COMMANDS = ("start", "stop", "toggle", "status")


@dataclass(frozen=True)
class PipelineResult:
    text: str
    used_llm: bool
    reason: str


Pipeline = Callable[[bytes], PipelineResult]
Sink = Callable[[str], None]


def build_pipeline(
    cfg: DictateConfig,
    env: Mapping[str, str] | None = None,
    *,
    transport: httpx.BaseTransport | None = None,
) -> Pipeline:
    """WAV bytes -> cleaned text. Vocabulary and keys are re-read per dictation, so edits apply at once."""
    e = os.environ if env is None else env

    def run(wav: bytes) -> PipelineResult:
        tcfg = resolve_transcribe_config(e, model=cfg.whisper_model, language=cfg.language)
        via_gateway = bool(e.get("WORDINK_GATEWAY_URL"))
        vocab = load_vocabulary(
            cfg.vocab_path,
            gateway=tcfg.endpoint if via_gateway else None,
            token=(e.get("WORDINK_ADMIN_TOKEN") or tcfg.token) if via_gateway else None,
            use_gateway=via_gateway,
            transport=transport,
        )
        raw = transcribe_audio(wav, tcfg, vocab, failed_dir=cfg.failed_dir, transport=transport)
        ccfg = CleanupConfig(
            api_key=e.get("GROQ_API_KEY"),
            model=cfg.llm_model,
            timeout=cfg.cleanup_timeout,
            min_words=cfg.min_words,
            tone=cfg.tone,
        )
        out = clean_transcript(raw, ccfg, vocab, transport=transport)
        return PipelineResult(out.text, out.used_llm, out.reason)

    return run


class LastTranscriptSink:
    """Writes each result to `last.txt` (0600, on the per-user runtime tmpfs). Insertion replaces this in U5."""

    def __init__(self, path: Path) -> None:
        self.path = path

    def __call__(self, text: str) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, tmp = tempfile.mkstemp(dir=self.path.parent, prefix=".last-")
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as fh:
                fh.write(text)
            os.chmod(tmp, 0o600)
            os.replace(tmp, self.path)
        except BaseException:
            Path(tmp).unlink(missing_ok=True)
            raise


class Daemon:
    def __init__(
        self,
        cfg: DictateConfig,
        pipeline: Pipeline,
        sink: Sink,
        *,
        recorder: Recorder | None = None,
        indicator: StateFile | None = None,
    ) -> None:
        self.cfg = cfg
        self.pipeline = pipeline
        self.sink = sink
        self.recorder = recorder or Recorder(
            cfg.record_command, min_seconds=cfg.min_seconds, tail_seconds=cfg.tail_seconds
        )
        self.indicator = indicator or StateFile(cfg.state_path)
        self.state = "idle"
        self._job: asyncio.Task[None] | None = None
        self._limit: asyncio.TimerHandle | None = None
        self._server: asyncio.Server | None = None

    # -- state ----------------------------------------------------------------------------------
    def _set(self, state: str, detail: str | None = None) -> None:
        self.state = state
        self.indicator.write(state, detail)

    def _reply(self, ok: bool = True, **extra: object) -> dict:
        return {"ok": ok, "state": self.state, **extra}

    # -- commands -------------------------------------------------------------------------------
    async def handle(self, command: str) -> dict:
        if command == "status":
            return self._reply()
        if command == "toggle":
            command = "stop" if self.state == "recording" else "start"
        if command == "start":
            return await self._start()
        if command == "stop":
            return self._stop()
        return self._reply(False, error=f"unknown command {command!r}; expected one of {', '.join(COMMANDS)}")

    async def _start(self) -> dict:
        if self.state == "recording":
            return self._reply(note="already recording")
        if self.state == "transcribing":
            return self._reply(False, error="still transcribing the last dictation")
        try:
            await self.recorder.start()
        except WordInkError as exc:
            self._set("error", str(exc))
            return self._reply(False, error=str(exc))
        self._set("recording")
        loop = asyncio.get_running_loop()
        self._limit = loop.call_later(self.cfg.max_seconds, self._on_limit)
        return self._reply()

    def _on_limit(self) -> None:
        if self.state == "recording":
            log.info("max recording length reached; stopping")
            self._stop()

    def _stop(self) -> dict:
        if self.state != "recording":
            return self._reply(note="not recording")
        if self._limit is not None:
            self._limit.cancel()
            self._limit = None
        self._set("transcribing")  # synchronously, so a second stop or start sees it
        self._job = asyncio.create_task(self._finish())
        return self._reply()

    async def _finish(self) -> None:
        try:
            wav = await self.recorder.stop()
            if wav is None:
                self._set("idle", "discarded: too short")
                return
            result = await asyncio.to_thread(self.pipeline, wav)
            log.info("dictation done (llm=%s, reason=%s, chars=%d)", result.used_llm, result.reason, len(result.text))
            if result.text:
                self.sink(result.text)
            self._set("idle")
        except TranscriptionError as exc:
            saved = f" (audio kept at {exc.saved_audio})" if exc.saved_audio else ""
            log.error("transcription failed: %s%s", exc, saved)
            self._set("error", f"{exc}{saved}")
        except WordInkError as exc:
            log.error("dictation failed: %s", exc)
            self._set("error", str(exc))
        except Exception as exc:  # keep the daemon alive whatever a stage throws
            log.exception("unexpected dictation failure")
            self._set("error", f"unexpected {type(exc).__name__}")

    # -- socket server --------------------------------------------------------------------------
    async def _client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            line = await asyncio.wait_for(reader.readline(), timeout=5)
            reply = await self.handle(line.decode("utf-8", "replace").strip().lower())
            writer.write((json.dumps(reply) + "\n").encode())
            await writer.drain()
        except (asyncio.TimeoutError, ConnectionError):
            pass
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError):
                await writer.wait_closed()

    def _claim_socket(self) -> None:
        path = self.cfg.socket_path
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(path.parent, 0o700)
        if path.exists():
            probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            probe.settimeout(1)
            try:
                probe.connect(str(path))
            except OSError:
                path.unlink(missing_ok=True)  # stale: nobody is listening
            else:
                raise WordInkError(f"wordink-dictd is already running (socket {path})")
            finally:
                probe.close()

    async def serve(self) -> None:
        self._claim_socket()
        self._server = await asyncio.start_unix_server(self._client, path=str(self.cfg.socket_path))
        os.chmod(self.cfg.socket_path, 0o600)
        self._set("idle")

    async def close(self) -> None:
        if self._limit is not None:
            self._limit.cancel()
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()
        await self.recorder.abort()
        if self._job is not None:
            await asyncio.gather(self._job, return_exceptions=True)
        self.cfg.socket_path.unlink(missing_ok=True)

    async def wait_idle(self) -> None:
        """Wait for an in-flight dictation to finish (tests and shutdown)."""
        if self._job is not None:
            await asyncio.gather(self._job, return_exceptions=True)


async def send_command(socket_path: Path, command: str, timeout: float = 3.0) -> dict:
    """One request to a running daemon; raises WordInkError when none is listening."""
    try:
        reader, writer = await asyncio.wait_for(asyncio.open_unix_connection(str(socket_path)), timeout)
    except (OSError, asyncio.TimeoutError) as exc:
        raise WordInkError(f"wordink-dictd is not running (socket {socket_path}); start it with `wordink-dictd run`") from exc
    try:
        writer.write(f"{command}\n".encode())
        await writer.drain()
        line = await asyncio.wait_for(reader.readline(), timeout)
        return json.loads(line)
    except (asyncio.TimeoutError, ValueError, ConnectionError) as exc:
        raise WordInkError("wordink-dictd did not answer") from exc
    finally:
        writer.close()
        with contextlib.suppress(ConnectionError):
            await writer.wait_closed()
