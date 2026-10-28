"""`wordink-dictd meeting ...`: start, stop, pause, resume, status, list, show, export, label, delete."""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

from .meeting import (
    EXPORT_FORMATS, SOURCES, Meeting, MeetingError, MeetingRunner, MeetingStore, _clock, capture_commands,
    default_meetings_dir, export,
)
from .transcribe import default_failed_dir, resolve_transcribe_config, transcribe_audio
from .vocab import default_vocab_path, read_terms

PID_FILE = "runner.pid"
STOP_WAIT_SECONDS = 90.0


def add_parser(sub: argparse._SubParsersAction) -> None:
    m = sub.add_parser("meeting", help="long-form meeting transcription with You/Remote speaker labels")
    ms = m.add_subparsers(dest="meeting_cmd", required=True)
    s = ms.add_parser("start", help="start recording a meeting in the background")
    s.add_argument("--title", default="")
    s.add_argument("--source", choices=SOURCES, default="both", help="mic, loopback (system audio) or both (default)")
    s.add_argument("--chunk-seconds", type=float, default=30.0)
    r = ms.add_parser("run", help=argparse.SUPPRESS)
    r.add_argument("--id", required=True)
    r.add_argument("--chunk-seconds", type=float, default=30.0)
    for name in ("stop", "pause", "resume"):
        ms.add_parser(name)
    ms.add_parser("status")
    ms.add_parser("list")
    for name in ("show", "delete"):
        ms.add_parser(name).add_argument("meeting", nargs="?", default="latest")
    e = ms.add_parser("export")
    e.add_argument("meeting", nargs="?", default="latest")
    e.add_argument("--format", choices=EXPORT_FORMATS, default="md")
    lb = ms.add_parser("label", help="rename a speaker, e.g. `meeting label latest Remote Alice`")
    lb.add_argument("meeting")
    lb.add_argument("speaker")
    lb.add_argument("name")


def _store() -> MeetingStore:
    return MeetingStore(default_meetings_dir())


def _active(store: MeetingStore) -> Meeting | None:
    return next((m for m in store.list() if m.state != "done" and _alive(m.dir / PID_FILE)), None)


def _alive(pidfile: Path) -> bool:
    try:
        os.kill(int(pidfile.read_text()), 0)
        return True
    except (OSError, ValueError):
        return False


def _signal_active(store: MeetingStore, sig: int) -> Meeting:
    m = _active(store)
    if m is None:
        raise MeetingError("no meeting is running")
    os.kill(int((m.dir / PID_FILE).read_text()), sig)
    return m


def _summary(m: Meeting) -> dict:
    return {"id": m.id, "title": m.title, "state": m.state, "started": m.started, "segments": len(m.segments),
            "duration": _clock(m.duration())}


async def _run(store: MeetingStore, meeting_id: str, chunk_seconds: float) -> int:
    meeting = store.get(meeting_id)
    cfg = resolve_transcribe_config()
    vocab = read_terms(default_vocab_path())
    failed = default_failed_dir()

    def transcriber(wav: bytes) -> str:
        return transcribe_audio(wav, cfg, vocab, failed_dir=failed)

    runner = MeetingRunner(store, meeting, capture_commands(meeting.source), transcriber, chunk_seconds=chunk_seconds)
    loop = asyncio.get_running_loop()
    loop.add_signal_handler(signal.SIGTERM, lambda: asyncio.ensure_future(runner.stop()))
    loop.add_signal_handler(signal.SIGINT, lambda: asyncio.ensure_future(runner.stop()))
    loop.add_signal_handler(signal.SIGUSR1, runner.pause)
    loop.add_signal_handler(signal.SIGUSR2, runner.resume)
    (meeting.dir / PID_FILE).write_text(str(os.getpid()))
    try:
        await runner.run()
    finally:
        (meeting.dir / PID_FILE).unlink(missing_ok=True)
    return 0


def handle(args: argparse.Namespace) -> int:
    store, cmd = _store(), args.meeting_cmd
    if cmd == "run":
        return asyncio.run(_run(store, args.id, args.chunk_seconds))
    if cmd == "start":
        if _active(store):
            raise MeetingError("a meeting is already running: stop it first")
        resolve_transcribe_config()  # fail now, not after the meeting, when the key is missing
        capture_commands(args.source)
        m = store.create(args.title, args.source)
        log = open(m.dir / "runner.log", "ab")  # noqa: SIM115
        subprocess.Popen(  # detached: outlives the shell and the keybind that started it
            [sys.executable, "-m", "wordink.desktop.cli", "meeting", "run", "--id", m.id, "--chunk-seconds", str(args.chunk_seconds)],
            stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True,
        )
        print(json.dumps(_summary(m)))
    elif cmd == "stop":
        m = _signal_active(store, signal.SIGTERM)
        deadline = time.monotonic() + STOP_WAIT_SECONDS  # the last chunk is still being transcribed
        while time.monotonic() < deadline and store.get(m.id).state != "done":
            time.sleep(0.5)
        print(json.dumps(_summary(store.get(m.id))))
    elif cmd in ("pause", "resume"):
        m = _signal_active(store, signal.SIGUSR1 if cmd == "pause" else signal.SIGUSR2)
        print(json.dumps({"id": m.id, "state": "paused" if cmd == "pause" else "recording"}))
    elif cmd == "status":
        m = _active(store)
        print(json.dumps(_summary(store.get(m.id)) if m else {"state": "idle"}))
    elif cmd == "list":
        for m in store.list():
            print(f"{m.id}\t{m.state}\t{_clock(m.duration())}\t{m.title}")
    elif cmd == "show":
        m = store.get(args.meeting)
        print(json.dumps({**_summary(m), "speakers": {s: m.name_of(s) for s in sorted({x.speaker for x in m.segments})}}, indent=2))
    elif cmd == "export":
        sys.stdout.write(export(store.get(args.meeting), args.format))
    elif cmd == "label":
        m = store.label(args.meeting, args.speaker, args.name)
        print(json.dumps({"id": m.id, "labels": m.labels}))
    elif cmd == "delete":
        print(json.dumps({"deleted": store.delete(args.meeting)}))
    return 0
