"""Meeting mode: long-form capture of the mic and the system audio, chunked Groq transcription,
and source-based speaker labels, a port of Voxtype's `meeting` feature.

Diarization is by input source, the same as Voxtype's default `simple` backend: what the mic
hears is "You", what the system loopback plays (the other side of a call) is "Remote". That
needs no ML model and no extra provider. `label` renames a speaker afterwards. Telling several
remote people apart (Voxtype's experimental `ml` backend) is a separate, later unit.
"""

from __future__ import annotations

import array
import asyncio
import contextlib
import json
import math
import os
import re
import shutil
import time
import uuid
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from ..client import WordInkError
from .recorder import SAMPLE_RATE, SAMPLE_WIDTH, pcm_to_wav

SOURCES = ("mic", "both", "loopback")
MIC_SPEAKER = "You"
LOOPBACK_SPEAKER = "Remote"
EXPORT_FORMATS = ("md", "txt", "json")
MIN_CHUNK_SECONDS = 1.0

Transcriber = Callable[[bytes], str]


class MeetingError(WordInkError):
    """A meeting could not start, was not found, or cannot be exported."""


def default_meetings_dir(env: dict[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    base = e.get("XDG_DATA_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".local" / "share")
    return Path(base) / "wordink" / "meetings"


def rms(pcm: bytes) -> float:
    """Root-mean-square level of s16le mono PCM, 0.0 to 1.0."""
    n = len(pcm) // SAMPLE_WIDTH
    if n == 0:
        return 0.0
    samples = array.array("h")
    samples.frombytes(pcm[: n * SAMPLE_WIDTH])
    return math.sqrt(sum(s * s for s in samples) / n) / 32768.0


def capture_commands(
    source: str, which: Callable[[str], str | None] = shutil.which
) -> list[tuple[str, str, tuple[str, ...]]]:
    """`(source name, speaker, command)` per stream, each writing raw s16le mono 16 kHz PCM to stdout."""
    if source not in SOURCES:
        raise MeetingError(f"meeting source must be one of {', '.join(SOURCES)}")
    out: list[tuple[str, str, tuple[str, ...]]] = []
    if which("parecord"):  # PipeWire's pulse shim; the default source and the default sink's monitor
        base = ("parecord", "--raw", "--rate=16000", "--channels=1", "--format=s16le")
        if source in ("mic", "both"):
            out.append(("mic", MIC_SPEAKER, (*base, "--device=@DEFAULT_SOURCE@")))
        if source in ("loopback", "both"):
            out.append(("loopback", LOOPBACK_SPEAKER, (*base, "--device=@DEFAULT_MONITOR@")))
        return out
    if which("pw-record"):
        base = ("pw-record", "--raw", "--rate", "16000", "--channels", "1", "--format", "s16")
        if source in ("mic", "both"):
            out.append(("mic", MIC_SPEAKER, (*base, "-")))
        if source in ("loopback", "both"):
            out.append(("loopback", LOOPBACK_SPEAKER, (*base, "-P", "stream.capture.sink=true", "-")))
        return out
    raise MeetingError("no capture tool found: install pipewire-pulse (parecord) or pipewire (pw-record)")


@dataclass
class Segment:
    start: float
    end: float
    source: str
    speaker: str
    text: str


@dataclass
class Meeting:
    id: str
    title: str
    started: str
    dir: Path
    ended: str | None = None
    state: str = "recording"  # recording | paused | done
    source: str = "both"
    labels: dict[str, str] = field(default_factory=dict)
    segments: list[Segment] = field(default_factory=list)

    @property
    def path(self) -> Path:
        return self.dir / "meeting.json"

    def name_of(self, speaker: str) -> str:
        return self.labels.get(speaker, speaker)

    def duration(self) -> float:
        return max((s.end for s in self.segments), default=0.0)

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id, "title": self.title, "started": self.started, "ended": self.ended,
            "state": self.state, "source": self.source, "labels": self.labels,
            "segments": [dict(vars(s)) for s in self.segments],
        }


def _stamp(now: datetime | None = None) -> str:
    return (now or datetime.now(timezone.utc)).strftime("%Y%m%dT%H%M%SZ")


def _slug(title: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")[:40]


class MeetingStore:
    """`<root>/<id>/meeting.json`, 0700 dirs and 0600 files, written atomically after every segment."""

    def __init__(self, root: Path) -> None:
        self.root = root

    def create(self, title: str = "", source: str = "both", *, now: datetime | None = None) -> Meeting:
        started = _stamp(now)
        slug = _slug(title)
        mid = f"{started}-{slug}" if slug else f"{started}-{uuid.uuid4().hex[:6]}"
        directory = self.root / mid
        directory.mkdir(parents=True, exist_ok=False, mode=0o700)
        meeting = Meeting(id=mid, title=title or "Untitled meeting", started=started, dir=directory, source=source)
        self.save(meeting)
        return meeting

    def save(self, meeting: Meeting) -> None:
        tmp = meeting.path.with_suffix(".tmp")
        fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            json.dump(meeting.to_dict(), fh, ensure_ascii=False, indent=2)
        os.replace(tmp, meeting.path)

    def _load(self, directory: Path) -> Meeting:
        try:
            d = json.loads((directory / "meeting.json").read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise MeetingError(f"cannot read meeting {directory.name}: {exc}") from exc
        return Meeting(
            id=d["id"], title=d["title"], started=d["started"], dir=directory, ended=d.get("ended"),
            state=d.get("state", "done"), source=d.get("source", "both"), labels=d.get("labels", {}),
            segments=[Segment(**s) for s in d.get("segments", [])],
        )

    def list(self) -> list[Meeting]:
        if not self.root.is_dir():
            return []
        dirs = sorted((p for p in self.root.iterdir() if (p / "meeting.json").is_file()), reverse=True)
        return [self._load(p) for p in dirs]

    def get(self, ref: str) -> Meeting:
        """`latest`, an exact id, or an unambiguous id prefix."""
        meetings = self.list()
        if not meetings:
            raise MeetingError("no meetings yet")
        if ref == "latest":
            return meetings[0]
        hits = [m for m in meetings if m.id == ref] or [m for m in meetings if m.id.startswith(ref)]
        if len(hits) != 1:
            raise MeetingError(f"no single meeting matches {ref!r}" if not hits else f"{ref!r} matches {len(hits)} meetings")
        return hits[0]

    def delete(self, ref: str) -> str:
        m = self.get(ref)
        if m.state != "done":
            raise MeetingError("that meeting is still running: stop it first")
        shutil.rmtree(m.dir)
        return m.id

    def label(self, ref: str, speaker: str, name: str) -> Meeting:
        m = self.get(ref)
        known = {s.speaker for s in m.segments}
        if speaker not in known:
            raise MeetingError(f"no speaker {speaker!r} in {m.id}: speakers are {', '.join(sorted(known)) or 'none yet'}")
        name = name.strip()
        if not name:
            raise MeetingError("a speaker name cannot be empty")
        m.labels[speaker] = name
        self.save(m)
        return m


def _clock(seconds: float) -> str:
    s = int(seconds)
    return f"{s // 3600:02d}:{s % 3600 // 60:02d}:{s % 60:02d}" if s >= 3600 else f"{s // 60:02d}:{s % 60:02d}"


def export(meeting: Meeting, fmt: str = "md") -> str:
    if fmt not in EXPORT_FORMATS:
        raise MeetingError(f"export format must be one of {', '.join(EXPORT_FORMATS)}")
    if fmt == "json":
        d = meeting.to_dict()
        for s in d["segments"]:
            s["speaker"] = meeting.name_of(s["speaker"])
        return json.dumps(d, ensure_ascii=False, indent=2)
    lines = [f"# {meeting.title}", "", f"{meeting.started}, {_clock(meeting.duration())}", ""] if fmt == "md" else []
    for s in meeting.segments:
        who = meeting.name_of(s.speaker)
        lines.append(f"**{who}** [{_clock(s.start)}]: {s.text}" if fmt == "md" else f"[{_clock(s.start)}] {who}: {s.text}")
        if fmt == "md":
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


class MeetingRunner:
    """Reads each capture stream, cuts it into `chunk_seconds` pieces, drops silent ones and
    transcribes the rest. Ends when every stream ends (stop terminates them) and the queue drains."""

    def __init__(
        self, store: MeetingStore, meeting: Meeting, streams: list[tuple[str, str, tuple[str, ...]]],
        transcriber: Transcriber, *, chunk_seconds: float = 30.0, vad_threshold: float = 0.01,
        max_seconds: float = 3 * 3600, clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self.store, self.meeting, self.streams, self.transcriber = store, meeting, streams, transcriber
        self.chunk_bytes = int(chunk_seconds * SAMPLE_RATE * SAMPLE_WIDTH)
        self.vad_threshold, self.max_seconds, self._clock = vad_threshold, max_seconds, clock
        self.paused = False
        self.errors: list[str] = []
        self._procs: list[asyncio.subprocess.Process] = []
        self._queue: asyncio.Queue[tuple[str, str, float, bytes] | None] = asyncio.Queue()
        self._stopping = False

    def _set_state(self, state: str) -> None:
        self.meeting.state = state
        self.store.save(self.meeting)

    def pause(self) -> None:
        self.paused = True
        self._set_state("paused")

    def resume(self) -> None:
        self.paused = False
        self._set_state("recording")

    async def stop(self) -> None:
        self._stopping = True
        for proc in self._procs:
            if proc.returncode is None:
                with contextlib.suppress(ProcessLookupError):
                    proc.terminate()

    async def _read(self, name: str, speaker: str, command: tuple[str, ...]) -> None:
        try:
            proc = await asyncio.create_subprocess_exec(
                *command, stdin=asyncio.subprocess.DEVNULL, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL
            )
        except OSError as exc:
            self.errors.append(f"cannot start {command[0]}: {exc}")
            return
        self._procs.append(proc)
        assert proc.stdout is not None
        buf = bytearray()
        offset = 0.0  # seconds of this stream already cut, counting paused time
        while data := await proc.stdout.read(8192):
            buf += data
            while len(buf) >= self.chunk_bytes:
                chunk, buf = bytes(buf[: self.chunk_bytes]), buf[self.chunk_bytes :]
                if not self.paused:
                    await self._queue.put((name, speaker, offset, chunk))
                offset += len(chunk) / (SAMPLE_RATE * SAMPLE_WIDTH)
            if offset >= self.max_seconds:
                await self.stop()
        if buf and not self.paused and len(buf) >= MIN_CHUNK_SECONDS * SAMPLE_RATE * SAMPLE_WIDTH:
            await self._queue.put((name, speaker, offset, bytes(buf)))
        with contextlib.suppress(ProcessLookupError):
            if proc.returncode is None:
                proc.terminate()
        await proc.wait()

    async def _work(self) -> None:
        while (item := await self._queue.get()) is not None:
            name, speaker, offset, pcm = item
            if rms(pcm) < self.vad_threshold:
                continue
            try:
                text = (await asyncio.to_thread(self.transcriber, pcm_to_wav(pcm))).strip()
            except WordInkError as exc:
                self.errors.append(str(exc))
                continue
            if text:
                end = offset + len(pcm) / (SAMPLE_RATE * SAMPLE_WIDTH)
                self.meeting.segments.append(Segment(round(offset, 1), round(end, 1), name, speaker, text))
                self.meeting.segments.sort(key=lambda s: (s.start, s.source))
                self.store.save(self.meeting)

    async def run(self) -> Meeting:
        worker = asyncio.create_task(self._work())
        try:
            await asyncio.gather(*(self._read(*s) for s in self.streams))
        finally:
            await self._queue.put(None)
            await worker
            self.meeting.ended = _stamp()
            self._set_state("done")
        if not self.meeting.segments and self.errors:
            raise MeetingError("; ".join(dict.fromkeys(self.errors)))
        return self.meeting
