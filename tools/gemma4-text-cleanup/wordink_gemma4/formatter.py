"""Training prompts aligned with desktop dictation post-processing."""

from __future__ import annotations

import json
from dataclasses import dataclass

PROTOCOL_PROMPT = """You transform dictated speech into replacement text. Return only the text that should be pasted. Do not add an explanation, label, alternative, or Markdown fence.

Numbers: keep every number the speaker said. Never drop, merge, reorder, round, or reinterpret a number, and never add thousands separators or other formatting to a number that the transcript already writes in digits.

Lists: unless the profile instructions say otherwise, when the speaker enumerates items (for example "point one ... point two ...", "first ... second ...", or "number one ... number two ..."), keep every item and write them as a numbered list, one item per line ("1. ...", "2. ..."), without the spoken cue words."""


@dataclass(frozen=True)
class VocabularyEntry:
    term: str
    sounds_like: tuple[str, ...] = ()


def vocabulary_lines(entries: list[VocabularyEntry]) -> str:
    if not entries:
        return ""
    parts: list[str] = []
    for entry in entries:
        if entry.sounds_like:
            aliases = ", ".join(entry.sounds_like)
            parts.append(f"{entry.term} (may be transcribed as {aliases})")
        else:
            parts.append(entry.term)
    terms = "; ".join(parts)
    return (
        "Vocabulary (spell these exactly when the speaker means them): "
        f"{terms}\n\n"
    )


def build_rewrite_prompt(
    *,
    profile_prompt: str,
    raw_transcript: str,
    application: str = "Unknown",
    browser_host: str = "(none)",
    vocabulary: list[VocabularyEntry] | None = None,
) -> str:
    """Same envelope the desktop processor sends to OpenCode or a local chat server."""
    vocab = vocabulary or []
    return (
        f"{PROTOCOL_PROMPT}\n\n"
        f"Profile instructions:\n{profile_prompt}\n\n"
        f"{vocabulary_lines(vocab)}"
        f"Foreground application: {application}\n"
        f"Browser host: {browser_host}\n\n"
        f"Raw transcript:\n<transcript>\n{raw_transcript}\n</transcript>"
    )


def to_prompt_completion(prompt: str, completion: str) -> dict[str, str]:
    return {"prompt": prompt, "completion": completion}


def to_chat_messages(prompt: str, completion: str) -> dict[str, list[dict[str, str]]]:
    return {
        "messages": [
            {"role": "user", "content": prompt},
            {"role": "assistant", "content": completion},
        ]
    }


def format_record(
    prompt: str,
    completion: str,
    *,
    style: str,
) -> dict:
    if style == "prompt_completion":
        return to_prompt_completion(prompt, completion)
    if style == "chat":
        return to_chat_messages(prompt, completion)
    raise ValueError(f"unknown dataset style: {style!r}")


def write_jsonl(path, records: list[dict]) -> None:
    with open(path, "w", encoding="utf-8") as file:
        for record in records:
            file.write(json.dumps(record, ensure_ascii=False))
            file.write("\n")