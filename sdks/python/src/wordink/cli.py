"""`wordink` CLI: transcribe audio files or dictate from the mic to stdout."""

from __future__ import annotations

import argparse
import io
import json
import sys
import wave
from pathlib import Path

from .client import (
    WordInkError,
    list_models,
    transcribe,
    vocab_add,
    vocab_list,
    vocab_remove,
)
from .flow import FlowTone, apply_flow_cleanup

TONE_CHOICES: tuple[FlowTone, ...] = ("casual", "work", "email", "code", "verbatim")

SAMPLE_RATE = 16000
MIMETYPES = {
    ".wav": "audio/wav",
    ".m4a": "audio/mp4",
    ".mp3": "audio/mpeg",
    ".mp4": "audio/mp4",
    ".webm": "audio/webm",
    ".ogg": "audio/ogg",
    ".flac": "audio/flac",
}


def _record(seconds: float | None) -> bytes:
    """Capture mic audio, return in-memory WAV bytes."""
    try:
        import sounddevice as sd
    except ImportError:
        raise WordInkError(
            "mic capture needs the 'mic' extra: uv tool install 'wordink[mic]'"
        ) from None
    import numpy as np

    chunks: list[np.ndarray] = []

    def collect(indata, frames, time, status):  # noqa: ARG001 - sd callback shape
        chunks.append(indata.copy())

    stream = sd.InputStream(
        samplerate=SAMPLE_RATE, channels=1, dtype="int16", callback=collect
    )
    with stream:
        if seconds:
            sd.sleep(int(seconds * 1000))
        else:
            print("recording — Enter or Ctrl-C to stop", file=sys.stderr)
            try:
                input()
            except (EOFError, KeyboardInterrupt):
                pass

    pcm = np.concatenate(chunks, axis=0) if chunks else np.zeros((0, 1), "int16")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(SAMPLE_RATE)
        wf.writeframes(pcm.tobytes())
    return buf.getvalue()


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="wordink",
        description="Dictation via a wordink-gateway (OpenAI-compatible endpoint)",
    )
    p.add_argument("--gateway", help="endpoint base URL (env WORDINK_GATEWAY_URL)")
    p.add_argument("--token", help="device token (env WORDINK_DEVICE_TOKEN)")
    sub = p.add_subparsers(dest="cmd", required=True)

    t = sub.add_parser("transcribe", help="transcribe an audio file to stdout")
    t.add_argument("file", type=Path)
    t.add_argument("--language", help="ISO-639 language hint, e.g. en")
    t.add_argument("--prompt", help="vocabulary/style hint passed upstream")
    t.add_argument(
        "--tone",
        choices=TONE_CHOICES,
        help="apply Wispr Flow tone preset (casual, work, email, code, verbatim)",
    )
    t.add_argument(
        "--flow-cleanup",
        action="store_true",
        help="run deterministic Wispr Flow post-processing (fillers, backtracking, lists, tone)",
    )

    d = sub.add_parser("dictate", help="record mic audio and transcribe it")
    d.add_argument("--seconds", type=float, help="fixed capture length")
    d.add_argument("--language")
    d.add_argument("--prompt")
    d.add_argument(
        "--tone",
        choices=TONE_CHOICES,
        help="apply Wispr Flow tone preset (casual, work, email, code, verbatim)",
    )
    d.add_argument(
        "--flow-cleanup",
        action="store_true",
        help="run deterministic Wispr Flow post-processing (fillers, backtracking, lists, tone)",
    )

    m = sub.add_parser("models", help="list models the gateway serves")
    m.set_defaults(models=True)

    v = sub.add_parser(
        "vocab",
        help="manage the gateway's runtime vocabulary (admin token: --token or WORDINK_ADMIN_TOKEN)",
    )
    vsub = v.add_subparsers(dest="vocab_cmd", required=True)
    vsub.add_parser("list", help="list the effective vocabulary (config + runtime)")
    va = vsub.add_parser("add", help="add terms to the runtime vocabulary")
    va.add_argument("terms", nargs="+")
    va.add_argument("--dry-run", action="store_true", help="report outcomes without persisting")
    vr = vsub.add_parser("remove", help="remove a runtime vocabulary term")
    vr.add_argument("term")
    vr.add_argument(
        "--via-post",
        action="store_true",
        help="send POST remove instead of DELETE (proxies that strip DELETE bodies)",
    )
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.cmd == "vocab":
            if args.vocab_cmd == "list":
                resp = vocab_list(gateway=args.gateway, token=args.token)
            elif args.vocab_cmd == "add":
                resp = vocab_add(
                    args.terms,
                    dry_run=args.dry_run,
                    gateway=args.gateway,
                    token=args.token,
                )
            else:
                resp = vocab_remove(
                    args.term,
                    via_post=args.via_post,
                    gateway=args.gateway,
                    token=args.token,
                )
            print(json.dumps(resp, indent=2))
            return 0
        if args.cmd == "models":
            for m in list_models(gateway=args.gateway, token=args.token):
                print(m)
            return 0
        if args.cmd == "transcribe":
            path: Path = args.file
            if not path.is_file():
                raise WordInkError(f"not a file: {path}")
            ctype = MIMETYPES.get(path.suffix.lower(), "application/octet-stream")
            text = transcribe(
                path,
                filename=path.name,
                content_type=ctype,
                gateway=args.gateway,
                token=args.token,
                language=args.language,
                prompt=args.prompt,
            )
            if args.flow_cleanup or args.tone:
                text = apply_flow_cleanup(text, {"tone": args.tone or "work"})
            print(text)
            return 0
        if args.cmd == "dictate":
            wav = _record(args.seconds)
            print("transcribing…", file=sys.stderr)
            text = transcribe(
                wav,
                filename="dictation.wav",
                gateway=args.gateway,
                token=args.token,
                language=args.language,
                prompt=args.prompt,
            )
            if args.flow_cleanup or args.tone:
                text = apply_flow_cleanup(text, {"tone": args.tone or "work"})
            print(text)
            return 0
    except WordInkError as exc:
        print(f"wordink: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
