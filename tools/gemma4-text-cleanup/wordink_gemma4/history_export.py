"""Opt-in exporter for local dictation history (never committed)."""

from __future__ import annotations

import json
from pathlib import Path


def load_history_pairs(path: Path) -> list[dict]:
    """
    Read WordInk/Hex `history.json` and return training rows.

    Uses `raw_text` as the STT-side transcript and `final_text` as the
    paste-ready completion. Skips raw-only entries and empty strings.
    """
    payload = json.loads(path.read_text(encoding="utf-8"))
    entries = payload.get("entries", payload if isinstance(payload, list) else [])
    rows: list[dict] = []
    for entry in entries:
        kind = entry.get("kind", "dictation")
        if kind in ("raw", "printed", "recovered"):
            continue
        raw = (entry.get("raw_text") or "").strip()
        final = (entry.get("final_text") or "").strip()
        if not raw or not final or raw == final:
            continue
        rows.append(
            {
                "raw": raw,
                "completion": final,
                "profile_prompt": "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.",
                "application": entry.get("application") or "Unknown",
                "vocabulary": [],
                "source": "history",
            }
        )
    return rows