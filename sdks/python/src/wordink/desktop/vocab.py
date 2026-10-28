"""Vocabulary for the dictation daemon: a local file merged with the gateway's runtime list.

The local file is plain text, one term per line (`#` starts a comment). Terms are matched
case-insensitively; the first spelling seen wins, so a user's casing survives merging.
"""

from __future__ import annotations

import logging
import os
import re
import tempfile
from collections.abc import Iterable
from pathlib import Path

import httpx

from ..client import WordInkError, vocab_list

log = logging.getLogger("wordink.dictd")

#: Whisper's prompt window is about 224 tokens; stay well inside it by characters.
MAX_PROMPT_CHARS = 800


def default_vocab_path(env: dict[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    base = e.get("XDG_CONFIG_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".config")
    return Path(base) / "wordink" / "vocabulary.txt"


def merge_terms(*groups: Iterable[str]) -> list[str]:
    """Union of term lists, de-duplicated case-insensitively, first spelling wins."""
    seen: set[str] = set()
    out: list[str] = []
    for group in groups:
        for raw in group:
            term = raw.strip()
            key = term.casefold()
            if term and key not in seen:
                seen.add(key)
                out.append(term)
    return out


def read_terms(path: Path) -> list[str]:
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return []
    lines = (line.split("#", 1)[0] for line in text.splitlines())
    return merge_terms(lines)


def _write_terms(path: Path, terms: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".vocab-")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write("".join(f"{t}\n" for t in terms))
        os.replace(tmp, path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def add_terms(path: Path, terms: Iterable[str]) -> list[str]:
    """Add terms to the local file; returns the terms that were newly added."""
    current = read_terms(path)
    known = {t.casefold() for t in current}
    added = [t for t in merge_terms(terms) if t.casefold() not in known]
    if added:
        _write_terms(path, current + added)
    return added


def remove_term(path: Path, term: str) -> bool:
    current = read_terms(path)
    kept = [t for t in current if t.casefold() != term.strip().casefold()]
    if len(kept) == len(current):
        return False
    _write_terms(path, kept)
    return True


def fetch_gateway_terms(
    *,
    gateway: str | None = None,
    token: str | None = None,
    transport: httpx.BaseTransport | None = None,
) -> list[str]:
    """Effective terms from a gateway's `/v1/vocabulary` (needs an admin or device token)."""
    body = vocab_list(gateway=gateway, token=token, transport=transport)
    data = body.get("data", [])
    return [item["term"] for item in data if isinstance(item, dict) and isinstance(item.get("term"), str)]


def load_vocabulary(
    path: Path,
    *,
    gateway: str | None = None,
    token: str | None = None,
    use_gateway: bool = False,
    transport: httpx.BaseTransport | None = None,
) -> list[str]:
    """Local terms, plus the gateway's when `use_gateway` is set. A gateway failure never blocks dictation."""
    local = read_terms(path)
    if not use_gateway:
        return local
    try:
        remote = fetch_gateway_terms(gateway=gateway, token=token, transport=transport)
    except WordInkError as exc:
        log.warning("gateway vocabulary unavailable, using local terms only: %s", exc)
        return local
    return merge_terms(local, remote)


def pack_prompt(terms: Iterable[str], max_chars: int = MAX_PROMPT_CHARS) -> str:
    """Comma-joined whole terms that fit `max_chars`; the earliest (local) terms win."""
    out: list[str] = []
    used = 0
    for term in merge_terms(terms):
        extra = len(term) + (2 if out else 0)
        if used + extra > max_chars:
            continue
        out.append(term)
        used += extra
    return ", ".join(out)


def apply_vocabulary(text: str, terms: Iterable[str]) -> str:
    """Rewrite whole-word, case-insensitive matches of each term to its canonical spelling."""
    out = text
    for term in sorted(merge_terms(terms), key=len, reverse=True):
        pattern = re.compile(rf"(?<!\w){re.escape(term)}(?!\w)", re.IGNORECASE)
        out = pattern.sub(lambda _m, t=term: t, out)
    return out
