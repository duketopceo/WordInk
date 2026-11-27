#!/usr/bin/env python3
"""Build Gemma 4 text-cleanup JSONL for local LoRA training."""

from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path

from .formatter import VocabularyEntry, build_rewrite_prompt, format_record, write_jsonl
from .history_export import load_history_pairs
from .synthetic import default_synthetic_pairs


def _vocab(rows: list[dict]) -> list[VocabularyEntry]:
    out: list[VocabularyEntry] = []
    for item in rows or []:
        out.append(
            VocabularyEntry(
                term=item["term"],
                sounds_like=tuple(item.get("sounds_like") or ()),
            )
        )
    return out


def build_training_rows(
    *,
    profile_prompt: str,
    synthetic: list[dict],
    history: list[dict],
    general_slice: float,
    seed: int,
    style: str,
) -> list[dict]:
    rng = random.Random(seed)
    records: list[dict] = []
    for row in synthetic + history:
        prompt = row.get("profile_prompt") or profile_prompt
        vocab = _vocab(row.get("vocabulary"))
        user_prompt = build_rewrite_prompt(
            profile_prompt=prompt,
            raw_transcript=row["raw"],
            application=row.get("application") or "Unknown",
            browser_host=row.get("browser_host") or "(none)",
            vocabulary=vocab,
        )
        records.append(format_record(user_prompt, row["completion"], style=style))

    general_path = Path(__file__).with_name("data") / "general_chat.jsonl"
    general: list[dict] = []
    if general_path.is_file():
        for line in general_path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line:
                general.append(json.loads(line))
    if general and general_slice > 0:
        count = max(1, int(len(records) * general_slice))
        count = min(count, len(general))
        picked = rng.sample(general, count)
        if style == "chat":
            records.extend(picked)
        else:
            for item in picked:
                messages = item["messages"]
                records.append(
                    format_record(messages[0]["content"], messages[1]["content"], style=style)
                )
    rng.shuffle(records)
    return records


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".local-gemma-training/train.jsonl"),
        help="JSONL path (default: .local-gemma-training/train.jsonl, gitignored)",
    )
    parser.add_argument(
        "--history",
        type=Path,
        help="Opt-in path to local history.json (never commit this file)",
    )
    parser.add_argument(
        "--profile-prompt",
        default="Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.",
    )
    parser.add_argument(
        "--general-chat-slice",
        type=float,
        default=0.1,
        help="Fraction of dictation rows to mix as general chat replay (0–0.15)",
    )
    parser.add_argument(
        "--style",
        choices=("chat", "prompt_completion"),
        default="chat",
        help="Gemma dataset shape",
    )
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args(argv)

    if not 0 <= args.general_chat_slice <= 0.15:
        print("general-chat-slice must be between 0 and 0.15", file=sys.stderr)
        return 2

    history_rows: list[dict] = []
    if args.history:
        if not args.history.is_file():
            print(f"history file not found: {args.history}", file=sys.stderr)
            return 2
        history_rows = load_history_pairs(args.history)

    records = build_training_rows(
        profile_prompt=args.profile_prompt,
        synthetic=default_synthetic_pairs(),
        history=history_rows,
        general_slice=args.general_chat_slice,
        seed=args.seed,
        style="chat" if args.style == "chat" else "prompt_completion",
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    write_jsonl(args.output, records)
    size = args.output.stat().st_size
    print(f"wrote {len(records)} records to {args.output} ({size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())