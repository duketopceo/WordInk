"""Groq Whisper transcription for the dictation daemon, over the existing WordInk client.

Default is direct-to-Groq (no gateway needed). Setting `WORDINK_GATEWAY_URL` sends the same
request to a wordink-gateway instead, which adds its own fallback chain and vocabulary.
"""

from __future__ import annotations

import os
import time
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

import httpx

from ..client import WordInkError, resolve_config, transcribe
from .vocab import pack_prompt

#: Groq's OpenAI-compatible root; `client.transcribe` appends `/v1/audio/transcriptions`.
GROQ_BASE = "https://api.groq.com/openai"
DEFAULT_WHISPER_MODEL = "whisper-large-v3-turbo"
RETRY_STATUSES = frozenset({429, 500, 502, 503, 504})
KEEP_FAILED_AUDIO = 10


@dataclass(frozen=True)
class TranscribeConfig:
    endpoint: str
    token: str = ""
    model: str = DEFAULT_WHISPER_MODEL
    language: str | None = None

    def __repr__(self) -> str:  # never let the token reach a log line
        return f"TranscribeConfig(endpoint={self.endpoint!r}, model={self.model!r}, language={self.language!r})"


class TranscriptionError(WordInkError):
    """A transcription that failed after its retry. `saved_audio` is the kept recording, if any."""

    saved_audio: Path | None = None


def resolve_transcribe_config(
    env: Mapping[str, str] | None = None,
    *,
    model: str | None = None,
    language: str | None = None,
) -> TranscribeConfig:
    """Gateway when `WORDINK_GATEWAY_URL` is set, otherwise Groq with `GROQ_API_KEY`."""
    e = os.environ if env is None else env
    gateway = e.get("WORDINK_GATEWAY_URL")
    if gateway:
        token = e.get("WORDINK_DEVICE_TOKEN") or e.get("WORDINK_ADMIN_TOKEN")
        if not token:
            raise WordInkError(
                "WORDINK_GATEWAY_URL is set but no device token is: set WORDINK_DEVICE_TOKEN "
                "(mint one with `wordink-gateway tokens create <label>`) or unset the gateway URL"
            )
        url, tok = resolve_config(gateway, token)
        return TranscribeConfig(url, tok, model or "wordink", language)
    key = e.get("GROQ_API_KEY")
    if not key:
        raise WordInkError(
            "GROQ_API_KEY is not set: put it in the service's EnvironmentFile "
            "(or set WORDINK_GATEWAY_URL and WORDINK_DEVICE_TOKEN to use a gateway)"
        )
    return TranscribeConfig(GROQ_BASE, key, model or DEFAULT_WHISPER_MODEL, language)


def default_failed_dir(env: Mapping[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    base = e.get("XDG_CACHE_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".cache")
    return Path(base) / "wordink" / "failed"


def save_failed_audio(wav: bytes, directory: Path, keep: int = KEEP_FAILED_AUDIO) -> Path:
    """Keep a failed recording for retry: 0700 dir, 0600 file, newest `keep` files only."""
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(directory, 0o700)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    path = directory / f"dictation-{stamp}.wav"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as fh:
        fh.write(wav)
    old = sorted(directory.glob("dictation-*.wav"))[:-keep] if keep > 0 else []
    for stale in old:
        stale.unlink(missing_ok=True)
    return path


def _retryable(exc: WordInkError) -> bool:
    return exc.status is None or exc.status in RETRY_STATUSES


def transcribe_audio(
    wav: bytes,
    cfg: TranscribeConfig,
    vocabulary: list[str] | None = None,
    *,
    failed_dir: Path | None = None,
    transport: httpx.BaseTransport | None = None,
    sleep: Callable[[float], None] = time.sleep,
    retry_delay: float = 0.5,
) -> str:
    """Transcribe WAV bytes once, retrying a single time on 429/5xx/network errors.

    On final failure the audio is kept under `failed_dir` (if given) and the raised
    `TranscriptionError.saved_audio` points at it. The token never appears in errors.
    """
    prompt = pack_prompt(vocabulary or []) or None
    last: WordInkError | None = None
    for attempt in range(2):
        try:
            return transcribe(
                wav,
                filename="dictation.wav",
                gateway=cfg.endpoint,
                token=cfg.token,
                language=cfg.language,
                prompt=prompt,
                model=cfg.model,
                transport=transport,
            ).strip()
        except WordInkError as exc:
            last = exc
            if attempt == 0 and _retryable(exc):
                sleep(retry_delay)
                continue
            break
    assert last is not None
    message = str(last.args[0]) if last.args else "transcription failed"
    if cfg.token:
        message = message.replace(cfg.token, "***")
    err = TranscriptionError(message, code=last.code, status=last.status)
    if failed_dir is not None:
        err.saved_audio = save_failed_audio(wav, failed_dir)
    raise err from None
