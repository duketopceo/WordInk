"""`wordink-dictd` command line: bind output, vocabulary management, and trigger against a live daemon."""

from __future__ import annotations

import asyncio
import json

import pytest

from wordink.desktop.cli import main
from wordink.desktop.daemon import Daemon, LastTranscriptSink, PipelineResult
from wordink.desktop.config import load_config
from wordink.desktop.recorder import Recorder

from dictd_helpers import FAKE_MIC, captured, short_tmp, wait_until


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

    async def scenario():
        daemon = Daemon(
            cfg,
            lambda wav: PipelineResult("hello from the daemon", False, "x"),
            LastTranscriptSink(cfg.last_path),
            recorder=Recorder(FAKE_MIC, min_seconds=0.2, tail_seconds=0.05),
        )
        await daemon.serve()
        loop = asyncio.get_running_loop()
        try:
            # main() runs its own event loop, so call it from a worker thread
            results["start"] = await loop.run_in_executor(None, main, ["trigger", "start"])
            await captured(daemon, 0.3)
            results["stop"] = await loop.run_in_executor(None, main, ["trigger", "stop"])
            await wait_until(lambda: daemon._job is not None)
            await daemon.wait_idle()
            results["status"] = await loop.run_in_executor(None, main, ["status"])
            results["last"] = await loop.run_in_executor(None, main, ["last"])
        finally:
            await daemon.close()

    asyncio.run(scenario())
    assert results == {"start": 0, "stop": 0, "status": 0, "last": 0}
    assert "hello from the daemon" in capsys.readouterr().out


def test_run_serves_until_sigterm_then_cleans_up_the_socket(env):
    import os
    import signal

    from wordink.desktop.cli import _run

    cfg = load_config()

    async def scenario():
        task = asyncio.create_task(_run(cfg))
        await wait_until(lambda: cfg.socket_path.exists())
        os.kill(os.getpid(), signal.SIGTERM)
        return await asyncio.wait_for(task, 10)

    assert asyncio.run(scenario()) == 0
    assert not cfg.socket_path.exists()


# --- Windows -------------------------------------------------------------------------------------


@pytest.fixture
def winenv(env, monkeypatch):
    monkeypatch.setenv("APPDATA", str(env / "AppData" / "Roaming"))
    monkeypatch.setenv("LOCALAPPDATA", str(env / "AppData" / "Local"))
    return env


def test_windows_binds_print_the_chords_not_hyprland_lines(winenv, capsys):
    assert main(["binds"], os_name="windows") == 0
    out = capsys.readouterr().out
    assert "Ctrl+Alt+D" in out and "Ctrl+Alt+S" in out and "bind" not in out


def test_windows_waybar_is_refused_politely(winenv, capsys):
    assert main(["waybar"], os_name="windows") == 1
    assert "Linux only" in capsys.readouterr().err


def test_set_key_on_linux_points_at_the_environment_file(winenv, capsys):
    assert main(["set-key"], os_name="linux") == 1
    assert "EnvironmentFile" in capsys.readouterr().err


def test_windows_run_hands_over_to_the_tray_app(winenv, monkeypatch):
    seen = {}
    monkeypatch.setattr("wordink.desktop.win.app.run_configured", lambda cfg, debug=False: seen.setdefault("os", cfg.hold_bind) and 0)
    assert main(["run"], os_name="windows") == 0
    assert seen["os"] == "Ctrl+Alt+D"


def test_windows_trigger_without_the_app_running_fails_with_a_hint(winenv, capsys):
    assert main(["trigger", "start"], os_name="windows") == 1
    assert "wordink-dictd" in capsys.readouterr().err


def test_windows_set_key_stores_without_echo(winenv, monkeypatch, capsys):
    called = []
    monkeypatch.setattr("wordink.desktop.win.secrets.set_key_interactive", lambda *a, **k: called.append(1))
    assert main(["set-key"], os_name="windows") == 0
    assert called and "gsk_" not in capsys.readouterr().out


def test_windows_doctor_runs_the_windows_checks(winenv, monkeypatch, capsys):
    from wordink.desktop.win import doctor as wd

    monkeypatch.setattr(wd, "real_probes", lambda cfg: wd.Probes())
    code = main(["doctor", "--json"], os_name="windows")
    assert code in (0, 1)
    assert json.loads(capsys.readouterr().out)
