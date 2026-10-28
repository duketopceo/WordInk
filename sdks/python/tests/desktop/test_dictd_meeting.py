"""Meeting mode: capture commands, silence gate, chunking, source labels, store, export, CLI."""

from __future__ import annotations

import asyncio
import json
import struct
import sys
from datetime import datetime, timezone

import pytest

from wordink.desktop import cli as dictd_cli
from wordink.desktop.meeting import (
    MeetingError, MeetingRunner, MeetingStore, capture_commands, export, rms,
)
from wordink.desktop.recorder import SAMPLE_RATE

NOW = datetime(2026, 10, 28, 9, 0, tzinfo=timezone.utc)


def _emit(*parts: tuple[str, float]) -> tuple[str, ...]:
    """A capture stand-in: writes `(kind, seconds)` parts of 16 kHz s16le PCM, then exits."""
    code = (
        "import struct, sys\n"
        f"for kind, secs in {list(parts)!r}:\n"
        "    v = 12000 if kind == 'loud' else 0\n"
        "    sys.stdout.buffer.write(struct.pack('<h', v) * int(16000 * secs))\n"
        "sys.stdout.buffer.flush()\n"
    )
    return (sys.executable, "-c", code)


def _loud(seconds: float) -> bytes:
    return struct.pack("<h", 12000) * int(SAMPLE_RATE * seconds)


def _quiet(seconds: float) -> bytes:
    return b"\x00\x00" * int(SAMPLE_RATE * seconds)


def test_rms_levels():
    assert rms(b"") == 0.0 and rms(_quiet(1)) == 0.0
    assert 0.3 < rms(_loud(1)) < 0.4


def test_capture_commands_per_source_prefer_parecord():
    which = lambda n: f"/usr/bin/{n}"  # noqa: E731
    both = capture_commands("both", which)
    assert [(n, sp) for n, sp, _ in both] == [("mic", "You"), ("loopback", "Remote")]
    assert "--device=@DEFAULT_SOURCE@" in both[0][2] and "--device=@DEFAULT_MONITOR@" in both[1][2]
    assert [n for n, *_ in capture_commands("mic", which)] == ["mic"]
    assert [n for n, *_ in capture_commands("loopback", which)] == ["loopback"]


def test_capture_commands_fall_back_to_pw_record_and_fail_clearly():
    only_pw = lambda n: "/usr/bin/pw-record" if n == "pw-record" else None  # noqa: E731
    assert "stream.capture.sink=true" in capture_commands("loopback", only_pw)[0][2]
    with pytest.raises(MeetingError, match="no capture tool"):
        capture_commands("both", lambda n: None)
    with pytest.raises(MeetingError, match="source must be"):
        capture_commands("speakers")


def test_runner_labels_by_source_skips_silence_and_orders_by_time(tmp_path):
    store = MeetingStore(tmp_path)
    m = store.create("Standup", now=NOW)
    heard: list[int] = []

    def transcriber(wav: bytes) -> str:
        heard.append(len(wav))
        return "hello from the mic" if len(heard) == 1 else "hello from the call"

    streams = [
        ("mic", "You", _emit(("loud", 2), ("quiet", 2))),
        ("loopback", "Remote", _emit(("quiet", 2), ("loud", 2))),
    ]
    runner = MeetingRunner(store, m, streams, transcriber, chunk_seconds=2.0)
    done = asyncio.run(runner.run())
    assert done.state == "done" and done.ended
    assert len(heard) == 2  # the two silent chunks never reached the transcriber
    pairs = {(s.speaker, s.source, s.start) for s in done.segments}
    assert pairs == {("You", "mic", 0.0), ("Remote", "loopback", 2.0)}
    assert [s.start for s in done.segments] == sorted(s.start for s in done.segments)
    assert store.get(m.id).segments[0].text  # persisted after each segment


def test_runner_keeps_going_when_one_chunk_fails_and_reports_if_all_fail(tmp_path):
    from wordink.client import WordInkError

    store = MeetingStore(tmp_path)
    m = store.create("Bad", now=NOW)

    def boom(wav: bytes) -> str:
        raise WordInkError("groq down")

    with pytest.raises(MeetingError, match="groq down"):
        asyncio.run(MeetingRunner(store, m, [("mic", "You", _emit(("loud", 2)))], boom, chunk_seconds=2.0).run())
    assert store.get(m.id).state == "done"  # still closed out, never left "recording"


def test_runner_reports_missing_capture_tool(tmp_path):
    store = MeetingStore(tmp_path)
    m = store.create(now=NOW)
    r = MeetingRunner(store, m, [("mic", "You", ("/nonexistent/capture-tool",))], lambda w: "x")
    with pytest.raises(MeetingError, match="cannot start"):
        asyncio.run(r.run())


def test_pause_drops_audio_but_keeps_timeline(tmp_path):
    store = MeetingStore(tmp_path)
    m = store.create(now=NOW)
    r = MeetingRunner(store, m, [("mic", "You", _emit(("loud", 2)))], lambda w: "kept", chunk_seconds=1.0)
    r.pause()
    assert store.get(m.id).state == "paused"
    done = asyncio.run(r.run())
    assert done.segments == []


def _seed(store):
    m = store.create("Weekly sync", now=NOW)
    from wordink.desktop.meeting import Segment

    m.segments = [Segment(0.0, 30.0, "mic", "You", "Morning."), Segment(30.0, 60.0, "loopback", "Remote", "Hi, ship it.")]
    store.save(m)
    return m


def test_label_rename_flows_into_every_export(tmp_path):
    store = MeetingStore(tmp_path)
    m = _seed(store)
    store.label("latest", "Remote", "Alice")
    got = store.get(m.id)
    assert "**Alice** [00:30]: Hi, ship it." in export(got, "md")
    assert "[00:30] Alice: Hi, ship it." in export(got, "txt")
    assert json.loads(export(got, "json"))["segments"][1]["speaker"] == "Alice"
    assert got.segments[1].speaker == "Remote"  # the stored source label is untouched
    with pytest.raises(MeetingError, match="no speaker 'Bob'"):
        store.label("latest", "Bob", "x")
    with pytest.raises(MeetingError, match="cannot be empty"):
        store.label("latest", "You", "  ")


def test_get_by_prefix_latest_and_ambiguity(tmp_path):
    store = MeetingStore(tmp_path)
    a = store.create("a", now=NOW)
    b = store.create("b", now=datetime(2026, 10, 29, tzinfo=timezone.utc))
    assert store.get("latest").id == b.id and store.get(a.id[:12]).id == a.id
    with pytest.raises(MeetingError, match="matches 2"):
        store.get("2026")
    with pytest.raises(MeetingError, match="no single meeting"):
        store.get("nope")


def test_store_files_are_private_and_delete_refuses_a_running_meeting(tmp_path):
    store = MeetingStore(tmp_path)
    m = store.create("x", now=NOW)
    assert oct(m.dir.stat().st_mode & 0o777) == "0o700" and oct(m.path.stat().st_mode & 0o777) == "0o600"
    with pytest.raises(MeetingError, match="still running"):
        store.delete(m.id)
    m.state = "done"
    store.save(m)
    assert store.delete("latest") == m.id and store.list() == []


def test_cli_list_show_export_label_roundtrip(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
    store = MeetingStore(tmp_path / "wordink" / "meetings")
    m = _seed(store)
    assert dictd_cli.main(["meeting", "list"]) == 0
    assert m.id in capsys.readouterr().out
    assert dictd_cli.main(["meeting", "label", "latest", "Remote", "Alice"]) == 0
    capsys.readouterr()
    assert dictd_cli.main(["meeting", "export", "latest", "--format", "txt"]) == 0
    assert "Alice: Hi, ship it." in capsys.readouterr().out
    assert dictd_cli.main(["meeting", "status"]) == 0
    assert json.loads(capsys.readouterr().out) == {"state": "idle"}


def test_cli_start_fails_before_recording_without_a_key(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
    for var in ("GROQ_API_KEY", "WORDINK_GATEWAY_URL"):
        monkeypatch.delenv(var, raising=False)
    assert dictd_cli.main(["meeting", "start"]) == 1
    assert "GROQ_API_KEY is not set" in capsys.readouterr().err
    assert not (tmp_path / "wordink" / "meetings").exists()
