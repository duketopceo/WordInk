"""Usage log and cost estimates for every billed Groq call (transcription and cleanup).

One JSON line per call in `~/.local/state/wordink/usage.jsonl` (0600): model, audio seconds,
billed seconds, token counts, latency and estimated USD. A line never holds a key or any
transcript text; `--debug` / `WORDINK_DEBUG=1` additionally logs text to stderr only.
"""

from __future__ import annotations

import contextvars
import json
import logging
import os
import tomllib
import wave
from collections import defaultdict
from collections.abc import Iterable, Mapping
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from io import BytesIO
from pathlib import Path
from typing import Any

log = logging.getLogger("wordink.usage")

#: Groq published prices, USD. Retrieved 2026-10-28 from
#: https://home.cloud.groq.io/pricing and https://console.groq.com/docs/models
#: Audio is billed per hour transcribed with a 10 second minimum per request; LLMs per million tokens.
PRICING_AS_OF = "2026-10-28"
PRICING_SOURCE = "https://home.cloud.groq.io/pricing and https://console.groq.com/docs/models (retrieved 2026-10-28)"
AUDIO_MIN_BILLED_SECONDS = 10.0
DEFAULT_PRICING: dict[str, dict[str, float]] = {
    "whisper-large-v3-turbo": {"audio_per_hour": 0.04},
    "whisper-large-v3": {"audio_per_hour": 0.111},
    "llama-3.1-8b-instant": {"input_per_m": 0.05, "output_per_m": 0.08},
    "llama-3.3-70b-versatile": {"input_per_m": 0.59, "output_per_m": 0.79},
    "openai/gpt-oss-20b": {"input_per_m": 0.075, "output_per_m": 0.30},
    "openai/gpt-oss-120b": {"input_per_m": 0.15, "output_per_m": 0.60},
}

#: Who the next call belongs to: "dictation" by default, a meeting id during a meeting.
session_var: contextvars.ContextVar[str] = contextvars.ContextVar("wordink_usage_session", default="dictation")


def debug_enabled(env: Mapping[str, str] | None = None) -> bool:
    e = os.environ if env is None else env
    return e.get("WORDINK_DEBUG", "").strip().lower() in ("1", "true", "yes", "on")


def default_usage_path(env: Mapping[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    if e.get("WORDINK_USAGE_LOG"):
        return Path(e["WORDINK_USAGE_LOG"])
    base = e.get("XDG_STATE_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".local" / "state")
    return Path(base) / "wordink" / "usage.jsonl"


def default_pricing_path(env: Mapping[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    base = e.get("XDG_CONFIG_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".config")
    return Path(base) / "wordink" / "pricing.toml"


def load_pricing(path: Path | None = None) -> tuple[dict[str, dict[str, float]], str]:
    """Defaults, with `~/.config/wordink/pricing.toml` (`as_of`, `[models."name"]`) layered on top."""
    table = {m: dict(p) for m, p in DEFAULT_PRICING.items()}
    as_of = PRICING_AS_OF
    p = path or default_pricing_path()
    if p.is_file():
        try:
            data = tomllib.loads(p.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as exc:
            log.warning("ignoring unreadable pricing file %s: %s", p, exc)
            return table, as_of
        as_of = str(data.get("as_of", "custom"))
        for model, prices in (data.get("models") or {}).items():
            if isinstance(prices, dict):
                table.setdefault(model, {}).update({k: float(v) for k, v in prices.items() if isinstance(v, (int, float))})
    return table, as_of


def wav_seconds(wav: bytes) -> float:
    try:
        with wave.open(BytesIO(wav)) as w:
            return w.getnframes() / float(w.getframerate())
    except (wave.Error, EOFError, ZeroDivisionError):
        return 0.0


def billed_seconds(audio_seconds: float) -> float:
    return max(audio_seconds, AUDIO_MIN_BILLED_SECONDS) if audio_seconds > 0 else 0.0


def estimate_usd(
    model: str, *, billed_s: float = 0.0, prompt_tokens: int = 0, completion_tokens: int = 0,
    pricing: Mapping[str, Mapping[str, float]] | None = None,
) -> float | None:
    """None when the model has no price (a gateway alias, an unknown model)."""
    price = (pricing or DEFAULT_PRICING).get(model)
    if not price:
        return None
    usd = billed_s / 3600.0 * price.get("audio_per_hour", 0.0)
    usd += prompt_tokens / 1e6 * price.get("input_per_m", 0.0) + completion_tokens / 1e6 * price.get("output_per_m", 0.0)
    return usd


def record(
    kind: str, model: str, *, latency_ms: float, ok: bool = True, status: int | None = None,
    audio_seconds: float = 0.0, prompt_tokens: int = 0, completion_tokens: int = 0,
    path: Path | None = None, pricing: Mapping[str, Mapping[str, float]] | None = None,
) -> dict[str, Any]:
    """Append one usage line. Never raises: a full disk must not break dictation. Failed calls cost 0."""
    billed = billed_seconds(audio_seconds) if ok else 0.0
    usd = estimate_usd(model, billed_s=billed, prompt_tokens=prompt_tokens if ok else 0,
                       completion_tokens=completion_tokens if ok else 0, pricing=pricing or load_pricing()[0])
    entry: dict[str, Any] = {
        "ts": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "session": session_var.get(), "kind": kind,
        "model": model, "ok": ok, "status": status, "audio_s": round(audio_seconds, 2), "billed_s": round(billed, 2),
        "prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens,
        "latency_ms": round(latency_ms), "est_usd": None if usd is None else round(usd, 7),
    }
    log.debug("usage %s", json.dumps(entry))
    target = path or default_usage_path()
    try:
        target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
        with os.fdopen(fd, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry) + "\n")
    except OSError as exc:
        log.warning("could not write the usage log %s: %s", target, exc)
    return entry


def read_entries(path: Path | None = None) -> list[dict[str, Any]]:
    target = path or default_usage_path()
    out: list[dict[str, Any]] = []
    try:
        lines = target.read_text(encoding="utf-8").splitlines()
    except FileNotFoundError:
        return out
    for line in lines:
        try:
            d = json.loads(line)
        except ValueError:
            continue  # a torn last line from a crash; skip it
        if isinstance(d, dict) and "ts" in d:
            out.append(d)
    return out


@dataclass
class Totals:
    calls: int = 0
    failed: int = 0
    unpriced: int = 0
    audio_s: float = 0.0
    billed_s: float = 0.0
    prompt_tokens: int = 0
    completion_tokens: int = 0
    usd: float = 0.0
    latency_ms: float = 0.0
    models: set[str] = field(default_factory=set)

    def add(self, e: dict[str, Any]) -> None:
        self.calls += 1
        self.failed += 0 if e.get("ok", True) else 1
        usd = e.get("est_usd")
        self.unpriced += 1 if usd is None else 0
        self.usd += usd or 0.0
        self.audio_s += e.get("audio_s", 0.0)
        self.billed_s += e.get("billed_s", 0.0)
        self.prompt_tokens += e.get("prompt_tokens", 0)
        self.completion_tokens += e.get("completion_tokens", 0)
        self.latency_ms += e.get("latency_ms", 0)
        self.models.add(e.get("model", "?"))

    def as_dict(self) -> dict[str, Any]:
        return {
            "calls": self.calls, "failed": self.failed, "unpriced_calls": self.unpriced,
            "audio_s": round(self.audio_s, 1), "billed_s": round(self.billed_s, 1),
            "prompt_tokens": self.prompt_tokens, "completion_tokens": self.completion_tokens,
            "est_usd": round(self.usd, 6), "avg_latency_ms": round(self.latency_ms / self.calls) if self.calls else 0,
            "models": sorted(self.models),
        }


def summarize(
    entries: Iterable[dict[str, Any]], *, days: int | None = None, session: str | None = None, now: datetime | None = None
) -> dict[str, Any]:
    cutoff = None
    if days is not None:
        cutoff = ((now or datetime.now(timezone.utc)) - timedelta(days=days - 1)).strftime("%Y-%m-%d")
    by_day: dict[str, Totals] = defaultdict(Totals)
    by_session: dict[str, Totals] = defaultdict(Totals)
    total = Totals()
    for e in entries:
        day = str(e["ts"])[:10]
        if (cutoff and day < cutoff) or (session and e.get("session") != session):
            continue
        by_day[day].add(e)
        by_session[str(e.get("session", "?"))].add(e)
        total.add(e)
    return {
        "pricing_as_of": load_pricing()[1],
        "total": total.as_dict(),
        "by_day": {d: t.as_dict() for d, t in sorted(by_day.items())},
        "by_session": {s: t.as_dict() for s, t in sorted(by_session.items())},
    }


def render(summary: dict[str, Any]) -> str:
    def row(label: str, t: dict[str, Any]) -> str:
        tok = f"{t['prompt_tokens']}/{t['completion_tokens']}"
        note = f"  ({t['unpriced_calls']} unpriced)" if t["unpriced_calls"] else ""
        return (f"{label:<34} {t['calls']:>5} {t['audio_s'] / 60:>8.1f} {t['billed_s'] / 60:>8.1f} "
                f"{tok:>13} {t['est_usd']:>10.4f}{note}")

    head = f"{'':<34} {'calls':>5} {'audio m':>8} {'billed m':>8} {'tok in/out':>13} {'est USD':>10}"
    lines = [head, "-" * len(head)]
    lines += [row(d, t) for d, t in summary["by_day"].items()] or ["(no usage recorded)"]
    if summary["by_session"]:
        lines += ["", "by session"] + [row(s[:34], t) for s, t in summary["by_session"].items()]
    lines += ["-" * len(head), row("total", summary["total"]), f"prices as of {summary['pricing_as_of']}; estimates, not an invoice"]
    return "\n".join(lines)
