"""`wordink` CLI: transcribe audio files or dictate from the mic to stdout."""

from __future__ import annotations

import argparse
import io
import sys
import wave
from pathlib import Path

from .client import WordInkError, list_models, transcribe

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

    d = sub.add_parser("dictate", help="record mic audio and transcribe it")
    d.add_argument("--seconds", type=float, help="fixed capture length")
    d.add_argument("--language")
    d.add_argument("--prompt")

    m = sub.add_parser("models", help="list models the gateway serves")
    m.set_defaults(models=True)
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.cmd == "models":
            for m in list_models(gateway=args.gateway, token=args.token):
                print(m)
            return 0
        if args.cmd == "transcribe":
            path: Path = args.file
            if not path.is_file():
                raise WordInkError(f"not a file: {path}")
            ctype = MIMETYPES.get(path.suffix.lower(), "application/octet-stream")
            print(
                transcribe(
                    path,
                    filename=path.name,
                    content_type=ctype,
                    gateway=args.gateway,
                    token=args.token,
                    language=args.language,
                    prompt=args.prompt,
                )
            )
            return 0
        if args.cmd == "dictate":
            wav = _record(args.seconds)
            print("transcribing…", file=sys.stderr)
            print(
                transcribe(
                    wav,
                    filename="dictation.wav",
                    gateway=args.gateway,
                    token=args.token,
                    language=args.language,
                    prompt=args.prompt,
                )
            )
            return 0
    except WordInkError as exc:
        print(f"wordink: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
