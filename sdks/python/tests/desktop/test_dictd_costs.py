"""Usage log, pricing and the `costs` command: the arithmetic, what is and is not written, debug logging."""

from __future__ import annotations

import io
import json
import logging
import stat
import wave
from datetime import datetime, timezone

import httpx
import pytest

from wordink.desktop import costs
from wordink.desktop.cleanup import CleanupConfig, clean_transcript
from wordink.desktop.cli import main
from wordink.desktop.transcribe import TranscribeConfig, TranscriptionError, transcribe_audio

KEY = "gsk_super_secret_key_value"
SECRET_WORDS = "the quarterly acquisition of Contoso"


def _wav(seconds: float) -> bytes:
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(b"\x00\x00" * int(16000 * seconds))
    return buf.getvalue()


def test_whisper_prices_match_groqs_published_per_hour_rates():
    assert costs.estimate_usd("whisper-large-v3-turbo", billed_s=3600) == pytest.approx(0.04)
    assert costs.estimate_usd("whisper-large-v3", billed_s=3600) == pytest.approx(0.111)


def test_audio_under_ten_seconds_is_billed_as_ten():
    assert costs.billed_seconds(2.0) == 10.0 and costs.billed_seconds(30.0) == 30.0 and costs.billed_seconds(0) == 0.0


def test_llm_cost_uses_input_and_output_token_prices():
    usd = costs.estimate_usd("llama-3.1-8b-instant", prompt_tokens=1_000_000, completion_tokens=1_000_000)
    assert usd == pytest.approx(0.05 + 0.08)


def test_unknown_model_is_unpriced_not_zero():
    assert costs.estimate_usd("wordink", billed_s=60) is None


def test_pricing_file_overrides_and_dates_the_table(tmp_path):
    f = tmp_path / "pricing.toml"
    f.write_text('as_of = "2027-01-02"\n[models."whisper-large-v3-turbo"]\naudio_per_hour = 0.05\n')
    table, as_of = costs.load_pricing(f)
    assert as_of == "2027-01-02" and table["whisper-large-v3-turbo"]["audio_per_hour"] == 0.05
    assert table["whisper-large-v3"]["audio_per_hour"] == 0.111  # untouched defaults remain
    assert costs.load_pricing(tmp_path / "missing.toml")[1] == costs.PRICING_AS_OF


def test_transcribe_logs_audio_billed_latency_and_cost_but_no_text_or_key():
    handler = lambda r: httpx.Response(200, json={"text": SECRET_WORDS})  # noqa: E731
    cfg = TranscribeConfig("https://api.groq.com/openai", KEY)
    assert transcribe_audio(_wav(4.0), cfg, transport=httpx.MockTransport(handler)) == SECRET_WORDS
    (e,) = costs.read_entries()
    assert (e["kind"], e["model"], e["ok"], e["session"]) == ("transcribe", "whisper-large-v3-turbo", True, "dictation")
    assert e["audio_s"] == 4.0 and e["billed_s"] == 10.0
    assert e["est_usd"] == pytest.approx(10 / 3600 * 0.04, abs=1e-7)
    raw = costs.default_usage_path().read_text()
    assert KEY not in raw and SECRET_WORDS not in raw and "Contoso" not in raw
    assert stat.S_IMODE(costs.default_usage_path().stat().st_mode) == 0o600


def test_a_failed_transcription_is_logged_at_zero_cost():
    cfg = TranscribeConfig("https://api.groq.com/openai", KEY)
    handler = lambda r: httpx.Response(401, json={"error": {"message": "bad key"}})  # noqa: E731
    with pytest.raises(TranscriptionError):
        transcribe_audio(_wav(20.0), cfg, transport=httpx.MockTransport(handler), sleep=lambda s: None)
    (e,) = costs.read_entries()
    assert e["ok"] is False and e["status"] == 401 and e["billed_s"] == 0.0 and e["est_usd"] == 0.0


def test_cleanup_logs_token_counts_from_the_response():
    body = {"choices": [{"message": {"content": "Hello there, my friend."}}], "usage": {"prompt_tokens": 120, "completion_tokens": 8}}
    out = clean_transcript("hello there my friend", CleanupConfig(api_key=KEY), transport=httpx.MockTransport(lambda r: httpx.Response(200, json=body)))
    assert out.used_llm
    (e,) = costs.read_entries()
    assert (e["kind"], e["model"], e["prompt_tokens"], e["completion_tokens"]) == ("cleanup", "llama-3.1-8b-instant", 120, 8)
    assert e["est_usd"] == pytest.approx(120 / 1e6 * 0.05 + 8 / 1e6 * 0.08, abs=1e-7)
    assert KEY not in costs.default_usage_path().read_text()


def test_no_usage_line_when_cleanup_never_calls_the_api():
    clean_transcript("hi", CleanupConfig(api_key=KEY))
    clean_transcript("hello there my friend", CleanupConfig(api_key=None))
    assert costs.read_entries() == []


def test_session_var_tags_calls_with_the_meeting_id():
    token = costs.session_var.set("20261028T090000Z-standup")
    try:
        costs.record("transcribe", "whisper-large-v3-turbo", latency_ms=5, audio_seconds=30)
    finally:
        costs.session_var.reset(token)
    assert costs.read_entries()[0]["session"] == "20261028T090000Z-standup"


def test_summary_groups_by_day_and_session_and_honours_filters():
    now = datetime(2026, 10, 28, 12, tzinfo=timezone.utc)
    rows = [
        {"ts": "2026-10-28T09:00:00Z", "session": "m1", "model": "whisper-large-v3-turbo", "audio_s": 30, "billed_s": 30, "est_usd": 0.0003333, "latency_ms": 400},
        {"ts": "2026-10-28T09:01:00Z", "session": "dictation", "model": "llama-3.1-8b-instant", "prompt_tokens": 100, "completion_tokens": 10, "est_usd": 0.0000058, "latency_ms": 200},
        {"ts": "2026-10-20T09:00:00Z", "session": "old", "model": "wordink", "audio_s": 5, "billed_s": 10, "est_usd": None, "latency_ms": 900},
    ]
    s = costs.summarize(rows, days=7, now=now)
    assert list(s["by_day"]) == ["2026-10-28"] and s["total"]["calls"] == 2 and s["total"]["est_usd"] == pytest.approx(0.0003391, abs=1e-6)
    assert costs.summarize(rows, days=30, now=now)["total"]["unpriced_calls"] == 1
    assert list(costs.summarize(rows, session="m1")["by_session"]) == ["m1"]
    assert "total" in costs.render(s) and "estimates, not an invoice" in costs.render(s)


def test_a_torn_last_line_is_skipped(tmp_path):
    p = tmp_path / "u.jsonl"
    p.write_text('{"ts": "2026-10-28T01:00:00Z", "session": "x"}\n{"ts": "2026-10')
    assert len(costs.read_entries(p)) == 1


def test_costs_command_prints_a_table_and_json(capsys):
    costs.record("transcribe", "whisper-large-v3", latency_ms=300, audio_seconds=3600)
    assert main(["costs"]) == 0
    assert "0.1110" in capsys.readouterr().out
    assert main(["costs", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["total"]["est_usd"] == pytest.approx(0.111)


def test_debug_logs_text_only_when_asked_and_never_the_key(caplog):
    cfg = TranscribeConfig("https://api.groq.com/openai", KEY)
    handler = lambda r: httpx.Response(200, json={"text": SECRET_WORDS})  # noqa: E731
    with caplog.at_level(logging.INFO):
        transcribe_audio(_wav(3), cfg, transport=httpx.MockTransport(handler))
    assert SECRET_WORDS not in caplog.text
    caplog.clear()
    with caplog.at_level(logging.DEBUG):
        transcribe_audio(_wav(3), cfg, transport=httpx.MockTransport(handler))
    assert SECRET_WORDS in caplog.text and "usage" in caplog.text
    assert KEY not in caplog.text


def test_debug_flag_and_env_turn_on_debug_logging(monkeypatch):
    assert costs.debug_enabled({"WORDINK_DEBUG": "1"}) and costs.debug_enabled({"WORDINK_DEBUG": "true"})
    assert not costs.debug_enabled({"WORDINK_DEBUG": "0"}) and not costs.debug_enabled({})
    main(["--debug", "costs"])
    assert logging.getLogger().level == logging.DEBUG
    assert logging.getLogger("httpx").level == logging.WARNING  # its debug lines can carry headers
    logging.getLogger().setLevel(logging.WARNING)
