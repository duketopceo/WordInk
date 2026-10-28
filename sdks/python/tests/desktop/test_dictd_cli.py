"""`wordink-dictd` command line: bind output, vocabulary management, and trigger against a live daemon."""

from __future__ import annotations

import asyncio
import json
import threading

import pytest

from wordink.desktop.cli import main
from wordink.desktop.daemon import Daemon, LastTranscriptSink, PipelineResult
from wordink.desktop.config import load_config
from wordink.desktop.recorder import Recorder

from dictd_helpers import FAKE_MIC, short_tmp


@pytest.fixture
def env(monkeypatch):
    root = short_tmp()
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(root / "run"))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(root / "cfg"))
    monkeypatch.setenv("XDG_CACHE_HOME", str(root / "cache"))
    monkeypatch.setenv("HOME", str(root))
    return root


def test_binds_prints_the_hyprland_lines(env, capsys):
    assert main(["binds"]) == 0
    out = capsys.readouterr().out.splitlines()
    assert out[0] == "bind = SUPER ALT, D, exec, wordink-dictd trigger start"
    assert out[1].startswith("bindr = SUPER ALT, D")
    assert out[2] == "bind = SUPER ALT, S, exec, wordink-dictd trigger toggle"


def test_vocab_add_list_remove(env, capsys):
    assert main(["vocab", "add", "WordInk", "Hyprland", "wordink"]) == 0
    assert json.loads(capsys.readouterr().out) == {"added": ["WordInk", "Hyprland"]}
    assert main(["vocab", "list"]) == 0
    assert capsys.readouterr().out.split() == ["WordInk", "Hyprland"]
    assert main(["vocab", "remove", "hyprland"]) == 0
    assert json.loads(capsys.readouterr().out) == {"removed": True}


def test_trigger_without_a_daemon_exits_nonzero_with_a_hint(env, capsys):
    assert main(["trigger", "start"]) == 1
    assert "wordink-dictd run" in capsys.readouterr().err


def test_last_before_any_dictation(env, capsys):
    assert main(["last"]) == 1
    assert "no dictation yet" in capsys.readouterr().err


def test_a_bad_config_is_reported_not_traceback(env, tmp_path, capsys):
    bad = tmp_path / "bad.toml"
    bad.write_text("[hotkey]\nhold = 'SUPER, D'\n")
    assert main(["--config", str(bad), "binds"]) == 1
    assert "wisp" in capsys.readouterr().err


def test_trigger_status_and_last_against_a_running_daemon(env, capsys):
    cfg = load_config()
    results: dict = {}

    def drive():
        # the CLI is synchronous (it runs asyncio itself), so drive it from a worker thread
        results["start"] = main(["trigger", "start"])
        import time

        time.sleep(0.5)
        results["stop"] = main(["trigger", "stop"])
        time.sleep(0.8)
        results["status"] = main(["status"])
        results["last"] = main(["last"])

    async def scenario():
        daemon = Daemon(
            cfg,
            lambda wav: PipelineResult("hello from the daemon", False, "x"),
            LastTranscriptSink(cfg.last_path),
            recorder=Recorder(FAKE_MIC, min_seconds=0.2, tail_seconds=0.05),
        )
        await daemon.serve()
        worker = threading.Thread(target=drive)
        worker.start()
        while worker.is_alive():
            await asyncio.sleep(0.05)
        await daemon.wait_idle()
        await daemon.close()

    asyncio.run(scenario())
    assert results == {"start": 0, "stop": 0, "status": 0, "last": 0}
    assert "hello from the daemon" in capsys.readouterr().out
