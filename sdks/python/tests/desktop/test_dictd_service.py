"""Service unit, waybar module, notifier and doctor."""

from __future__ import annotations

import configparser
import io
import json
import re
import socket
import time
from pathlib import Path

import pytest

from wordink.desktop.config import load_config
from wordink.desktop.doctor import exit_code, render, run_checks, to_json
from wordink.desktop.indicator import StateFile
from wordink.desktop.notify import notify
from wordink.desktop.waybar import daemon_alive, follow, payload, snapshot

from dictd_helpers import short_tmp
from test_dictd_inject import Rig

EXAMPLES = Path(__file__).resolve().parents[4] / "examples" / "omarchy-dictation"


# -- the unit file -----------------------------------------------------------------------------
def _unit() -> configparser.ConfigParser:
    cp = configparser.ConfigParser(strict=False, interpolation=None)
    cp.optionxform = str  # type: ignore[assignment]
    cp.read(EXAMPLES / "wordink-dictd.service")
    return cp


def test_unit_runs_the_daemon_and_restarts_it_without_holding_secrets():
    cp = _unit()
    assert cp["Service"]["ExecStart"] == "%h/.local/bin/wordink-dictd run"
    assert cp["Service"]["Restart"] == "on-failure"
    assert cp["Service"]["EnvironmentFile"] == "-%h/.config/wordink/dictd.env"
    assert cp["Install"]["WantedBy"] == "graphical-session.target"
    assert not re.search(r"gsk_|GROQ_API_KEY=\S", (EXAMPLES / "wordink-dictd.service").read_text())


def test_waybar_module_is_valid_jsonc_pointing_at_the_follow_command():
    text = re.sub(r"^\s*//.*$", "", (EXAMPLES / "waybar-module.jsonc").read_text(), flags=re.M)
    mod = json.loads(text)["custom/wordink"]
    assert mod["exec"] == "wordink-dictd waybar --follow" and mod["return-type"] == "json"
    css = (EXAMPLES / "waybar-style.css").read_text()
    for cls in ("recording", "transcribing", "error", "off"):
        assert f"#custom-wordink.{cls}" in css


# -- waybar payloads ---------------------------------------------------------------------------
@pytest.mark.parametrize("state", ["idle", "recording", "transcribing", "error"])
def test_payload_class_follows_state(state):
    p = payload({"state": state}, alive=True)
    assert p["class"] == state and p["alt"] == state and p["text"]


def test_a_dead_daemon_shows_off_even_if_the_state_file_says_recording():
    assert payload({"state": "recording"}, alive=False)["class"] == "off"


def test_error_detail_is_in_the_tooltip():
    assert "no mic" in payload({"state": "error", "detail": "no mic"}, alive=True)["tooltip"]


def test_unknown_state_is_shown_as_error():
    assert payload({"state": "weird"}, alive=True)["class"] == "error"


def _listen(path: Path) -> socket.socket:
    srv = socket.socket(socket.AF_UNIX)
    srv.bind(str(path))
    srv.listen(128)
    return srv


def test_snapshot_and_alive_probe():
    root = short_tmp()
    sock = root / "d.sock"
    StateFile(root / "state").write("recording")
    assert not daemon_alive(sock)
    assert snapshot(root / "state", sock)["class"] == "off"
    srv = _listen(sock)
    try:
        assert daemon_alive(sock)
        assert snapshot(root / "state", sock)["class"] == "recording"
    finally:
        srv.close()


def test_follow_prints_only_on_change():
    root = short_tmp()
    sock = root / "d.sock"
    srv = _listen(sock)
    sf = StateFile(root / "state")
    sf.write("idle")
    out = io.StringIO()
    ticks = {"n": 0}

    def sleep(_s: float) -> None:
        ticks["n"] += 1
        if ticks["n"] == 2:
            sf.write("recording")

    try:
        follow(root / "state", sock, out, should_stop=lambda: ticks["n"] >= 4, sleep=sleep)
    finally:
        srv.close()
    classes = [json.loads(l)["class"] for l in out.getvalue().splitlines()]
    assert classes == ["idle", "recording"]


# -- notifier ----------------------------------------------------------------------------------
def test_notify_runs_notify_send_and_survives_its_absence(tmp_path, monkeypatch):
    monkeypatch.setenv("FAKE", str(tmp_path))
    rig = Rig(tmp_path, ["notify-send"])
    assert notify("hello", urgency="critical", path=str(rig.bin))
    for _ in range(100):
        if rig.log:
            break
        time.sleep(0.05)
    assert rig.log and "-u critical" in rig.log[0] and "hello" in rig.log[0]
    assert notify("hello", path=str(tmp_path / "empty")) is False


def test_the_daemon_notifies_when_dictation_fails():
    import asyncio

    from wordink.client import WordInkError
    from wordink.desktop.daemon import Daemon
    from dictd_helpers import captured
    from test_dictd_daemon import make_cfg, cmd, SpyState

    root = short_tmp()
    cfg = make_cfg(root)
    seen: list[str] = []

    def boom(wav):
        raise WordInkError("GROQ_API_KEY is not set")

    async def scenario():
        daemon = Daemon(cfg, boom, lambda t: None, indicator=SpyState(cfg.state_path), notifier=seen.append)
        await daemon.serve()
        try:
            await cmd(cfg, "start")
            await captured(daemon)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
        finally:
            await daemon.close()

    asyncio.run(scenario())
    assert seen == ["Dictation failed: GROQ_API_KEY is not set"]


# -- doctor ------------------------------------------------------------------------------------
HYPR = {
    "hyprctl": 'cat "$FAKE/binds.json"',
    "pw-record": "exit 0",
    "wtype": "exit 0",
    "wl-copy": "cat > /dev/null",
    "wl-paste": "exit 0",
    "notify-send": "exit 0",
}


def _doctor_rig(tmp_path, monkeypatch, binds: list[dict], tools=None) -> tuple[Rig, Path]:
    import stat
    import textwrap

    monkeypatch.setenv("FAKE", str(tmp_path))
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    for name in tools or HYPR:
        exe = bin_dir / name
        exe.write_text(f"#!/bin/sh\n{textwrap.dedent(HYPR[name])}\n")
        exe.chmod(exe.stat().st_mode | stat.S_IXUSR)
    (tmp_path / "binds.json").write_text(json.dumps(binds))
    cfg = load_config(tmp_path / "none.toml", {"HOME": str(tmp_path), "XDG_RUNTIME_DIR": str(short_tmp())})
    return bin_dir, cfg


def _by_name(checks):
    return {c.name: c for c in checks}


GOOD_ENV = {"GROQ_API_KEY": "gsk_secret_value", "WAYLAND_DISPLAY": "wayland-1"}
SUPER_ALT = 64 + 8


def test_doctor_all_good_with_our_binds_installed(tmp_path, monkeypatch):
    binds = [
        {"modmask": SUPER_ALT, "key": "D", "dispatcher": "exec", "arg": "wordink-dictd trigger start"},
        {"modmask": SUPER_ALT, "key": "S", "dispatcher": "exec", "arg": "wordink-dictd trigger toggle"},
        {"modmask": 64, "key": "D", "dispatcher": "exec", "arg": "wispd trigger"},
    ]
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, binds)
    checks = _by_name(run_checks(cfg, GOOD_ENV, path=str(bin_dir)))
    assert checks["hold bind"].status == "ok" and checks["toggle bind"].status == "ok"
    assert checks["credentials"].status == "ok" and checks["microphone tool"].detail == "pw-record"
    assert checks["daemon"].status == "warn"  # not running in this test


def test_doctor_reports_a_foreign_bind_on_our_key(tmp_path, monkeypatch):
    binds = [{"modmask": SUPER_ALT, "key": "d", "dispatcher": "exec", "arg": "some-other-tool"}]
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, binds)
    checks = run_checks(cfg, GOOD_ENV, path=str(bin_dir))
    hold = _by_name(checks)["hold bind"]
    assert hold.status == "fail" and "some-other-tool" in hold.detail and hold.fix
    assert exit_code(checks) == 1


def test_doctor_warns_when_binds_are_not_installed_yet(tmp_path, monkeypatch):
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, [])
    checks = _by_name(run_checks(cfg, GOOD_ENV, path=str(bin_dir)))
    assert checks["hold bind"].status == "warn" and "binds" in checks["hold bind"].fix


def test_doctor_missing_key_fails_but_never_prints_a_key(tmp_path, monkeypatch):
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, [])
    checks = run_checks(cfg, {"WAYLAND_DISPLAY": "w"}, path=str(bin_dir))
    assert _by_name(checks)["credentials"].status == "fail" and exit_code(checks) == 1
    full = run_checks(cfg, GOOD_ENV, path=str(bin_dir))
    assert "gsk_secret_value" not in render(full) and "gsk_secret_value" not in to_json(full)


def test_doctor_gateway_mode_checks_the_device_token(tmp_path, monkeypatch):
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, [])
    env = {"WORDINK_GATEWAY_URL": "http://127.0.0.1:8941", "WAYLAND_DISPLAY": "w"}
    checks = _by_name(run_checks(cfg, env, path=str(bin_dir)))
    assert checks["credentials"].status == "fail" and "device token missing" in checks["credentials"].detail
    ok = _by_name(run_checks(cfg, {**env, "WORDINK_DEVICE_TOKEN": "t"}, path=str(bin_dir)))
    assert ok["credentials"].status == "ok"


def test_doctor_with_no_tools_fails_on_the_microphone_and_degrades_the_rest(tmp_path, monkeypatch):
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, [], tools=["hyprctl"])
    checks = _by_name(run_checks(cfg, GOOD_ENV, path=str(bin_dir)))
    assert checks["microphone tool"].status == "fail" and checks["typing tool"].status == "warn"
    assert checks["wl-copy"].status == "warn" and checks["notify-send"].status == "warn"


def test_doctor_sees_a_running_daemon(tmp_path, monkeypatch):
    import asyncio

    from wordink.desktop.daemon import Daemon, PipelineResult
    from test_dictd_daemon import make_cfg

    bin_dir, _ = _doctor_rig(tmp_path, monkeypatch, [])
    cfg = make_cfg(short_tmp())

    async def scenario():
        daemon = Daemon(cfg, lambda w: PipelineResult("x", False, "x"), lambda t: None)
        await daemon.serve()
        try:
            # run_checks is synchronous and runs its own loop, so call it from a worker thread
            return await asyncio.get_running_loop().run_in_executor(None, lambda: run_checks(cfg, GOOD_ENV, path=str(bin_dir)))
        finally:
            await daemon.close()

    daemon_check = _by_name(asyncio.run(scenario()))["daemon"]
    assert daemon_check.status == "ok" and "idle" in daemon_check.detail


def test_cli_doctor_and_waybar_commands(tmp_path, monkeypatch, capsys):
    from wordink.desktop.cli import main

    monkeypatch.setenv("XDG_RUNTIME_DIR", str(short_tmp()))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "cfg"))
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("PATH", str(tmp_path / "nothing"))
    monkeypatch.delenv("GROQ_API_KEY", raising=False)
    monkeypatch.delenv("WORDINK_GATEWAY_URL", raising=False)
    assert main(["waybar"]) == 0
    assert json.loads(capsys.readouterr().out)["class"] == "off"
    assert main(["doctor", "--json"]) == 1
    names = {c["name"]: c["status"] for c in json.loads(capsys.readouterr().out)}
    assert names["credentials"] == "fail" and names["microphone tool"] == "fail"


def test_doctor_recognises_lua_binds_by_description(tmp_path, monkeypatch):
    """`hyprctl binds -j` shows a Lua bind's command as `__lua N`; the description is what identifies ours."""
    binds = [
        {"modmask": SUPER_ALT, "key": "D", "dispatcher": "__lua", "arg": "303", "description": "WordInk dictation (hold)"},
        {"modmask": SUPER_ALT, "key": "S", "dispatcher": "__lua", "arg": "304", "description": "WordInk dictation (toggle)"},
    ]
    bin_dir, cfg = _doctor_rig(tmp_path, monkeypatch, binds)
    checks = _by_name(run_checks(cfg, GOOD_ENV, path=str(bin_dir)))
    assert checks["hold bind"].status == "ok" and checks["toggle bind"].status == "ok"
    other = [{"modmask": SUPER_ALT, "key": "D", "dispatcher": "__lua", "arg": "9", "description": "Something else"}]
    (tmp_path / "two").mkdir()
    bin_dir, cfg = _doctor_rig(tmp_path / "two", monkeypatch, other)
    assert _by_name(run_checks(cfg, GOOD_ENV, path=str(bin_dir)))["hold bind"].status == "fail"
