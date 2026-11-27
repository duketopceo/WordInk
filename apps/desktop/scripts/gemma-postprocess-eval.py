#!/usr/bin/env python3
"""
Score a local OpenAI-compatible cleanup model on the vocabulary benchmark.

Runs each clip's reference transcript through POST /v1/chat/completions, then
scores with vocabulary-eval.py (term recall + false-replacement gates).
"""

from __future__ import annotations

import argparse
import json
import runpy
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests/fixtures/vocabulary/manifest.json"
SCORE = runpy.run_path(str(Path(__file__).with_name("vocabulary-eval.py")))


def cleanup(base_url: str, model: str, prompt: str) -> str:
    url = base_url.rstrip("/") + "/v1/chat/completions"
    body = json.dumps(
        {
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        payload = json.loads(response.read().decode("utf-8"))
    return payload["choices"][0]["message"]["content"].strip()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--model", default="gemma4-cleanup")
    parser.add_argument("--manifest", type=Path, default=FIXTURE)
    parser.add_argument(
        "--profile-prompt",
        default="Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.",
    )
    args = parser.parse_args()
    if not args.manifest.is_file():
        print(f"manifest missing: {args.manifest}", file=sys.stderr)
        return 2

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    results: list[dict] = []
    repo = Path(__file__).resolve().parents[3]
    tools = runpy.run_path(
        str(repo / "tools/gemma4-text-cleanup/wordink_gemma4/formatter.py")
    )
    build_prompt = tools["build_rewrite_prompt"]

    for clip in manifest["clips"]:
        expected = clip.get("expected") or clip.get("transcript") or ""
        prompt = build_prompt(
            profile_prompt=args.profile_prompt,
            raw_transcript=expected,
            application="Benchmark",
        )
        try:
            text = cleanup(args.base_url, args.model, prompt)
        except (urllib.error.URLError, KeyError, json.JSONDecodeError) as error:
            print(f"{clip['id']}: request failed: {error}", file=sys.stderr)
            return 1
        results.append({"type": "clip", "id": clip["id"], "transcript": text, "expected": expected})

    report = SCORE["score"](manifest, results)
    print(json.dumps(report, indent=2))
    guard = report.get("guard", {})
    target = report.get("target", {})
    if guard.get("false_replacement_rate", 1) > 0.005:
        print("false_replacement_rate gate failed (>0.5%)", file=sys.stderr)
        return 1
    if target.get("term_recall") is not None and target["term_recall"] < 0.7:
        print("term_recall gate failed (<70%)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())