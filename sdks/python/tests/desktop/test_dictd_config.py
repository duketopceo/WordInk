"""dictate.toml loading and runtime paths."""

from __future__ import annotations

from pathlib import Path

import pytest

from wordink.client import WordInkError
from wordink.desktop.config import load_config


def _env(tmp_path: Path) -> dict[str, str]:
    return {
        "XDG_RUNTIME_DIR": str(tmp_path / "run"),
        "XDG_CONFIG_HOME": str(tmp_path / "cfg"),
        "XDG_CACHE_HOME": str(tmp_path / "cache"),
        "HOME": str(tmp_path),
    }


def _write(tmp_path: Path, body: str) -> Path:
    path = tmp_path / "dictate.toml"
    path.write_text(body, encoding="utf-8")
    return path


def test_defaults_without_a_file(tmp_path):
    cfg = load_config(tmp_path / "missing.toml", _env(tmp_path))
    assert cfg.hold_bind == "SUPER ALT, D" and cfg.toggle_bind == "SUPER ALT, S"
    assert cfg.whisper == "whisper-large-v3-turbo" and cfg.llm_model == "llama-3.1-8b-instant"
    assert cfg.tone == "work" and cfg.record_command is None
    assert cfg.socket_path == tmp_path / "run" / "wordink" / "dictd.sock"
    assert cfg.state_path.name == "state" and cfg.last_path.name == "last.txt"
    assert cfg.vocab_path == tmp_path / "cfg" / "wordink" / "vocabulary.txt"
    assert cfg.failed_dir == tmp_path / "cache" / "wordink" / "failed"


def test_file_overrides(tmp_path):
    path = _write(
        tmp_path,
        """
[hotkey]
hold = "SUPER SHIFT, V"
toggle = "SUPER SHIFT, B"
[transcribe]
model = "whisper-large-v3"
language = "en"
[cleanup]
model = "llama-3.3-70b-versatile"
tone = "casual"
timeout = 5
min_words = 2
[record]
command = ["arecord", "-q"]
min_seconds = 0.5
max_seconds = 30
tail_seconds = 0
""",
    )
    cfg = load_config(path, _env(tmp_path))
    assert (cfg.hold_bind, cfg.toggle_bind) == ("SUPER SHIFT, V", "SUPER SHIFT, B")
    assert (cfg.whisper, cfg.language) == ("whisper-large-v3", "en")
    assert (cfg.llm_model, cfg.tone, cfg.cleanup_timeout, cfg.min_words) == ("llama-3.3-70b-versatile", "casual", 5.0, 2)
    assert cfg.record_command == ("arecord", "-q")
    assert (cfg.min_seconds, cfg.max_seconds, cfg.tail_seconds) == (0.5, 30.0, 0.0)


@pytest.mark.parametrize(
    ("body", "message"),
    [
        ("[nope]\nx = 1", "unknown section"),
        ("[hotkey]\nhod = 'SUPER, X'", "unknown key"),
        ("[cleanup]\ntone = 'shouty'", "tone must be one of"),
        ("[cleanup]\ntimeout = 0", "timeout must be a number"),
        ("[record]\nmax_seconds = 'long'", "max_seconds"),
        ("[record]\ncommand = 'arecord'", "list of strings"),
        ("[hotkey]\nhold = 'SUPER, D'", "wisp"),
        ("[hotkey]\nhold = 'SUPER ALT, S'", "different"),
        ("[hotkey", "cannot read"),
    ],
)
def test_bad_files_fail_loudly(tmp_path, body, message):
    with pytest.raises(WordInkError, match=message):
        load_config(_write(tmp_path, body), _env(tmp_path))


def test_runtime_dir_falls_back_without_xdg(tmp_path):
    env = {"HOME": str(tmp_path)}
    assert load_config(tmp_path / "x.toml", env).runtime_dir.name.startswith("wordink-")
