"""Windows doctor against fake probes (plan W7c)."""

from __future__ import annotations

import json

from pathlib import Path

from wordink.desktop.config import DictateConfig
from wordink.desktop.doctor import exit_code, render, to_json
from wordink.desktop.win.doctor import Probes, run_checks

KEY = "gsk_do_not_leak_me"


def cfg(**kw):
    return DictateConfig(runtime_dir=Path("r"), vocab_path=Path("v"), failed_dir=Path("f"), hold_bind="Ctrl+Alt+D", toggle_bind="Ctrl+Alt+S", **kw)


def good(**over):
    base = dict(
        stored_key=lambda: True, input_device=lambda: "Microphone (USB)", hotkey_clashes=lambda: [], task_state=lambda: "Ready",
        process_command_lines=lambda: [r'"C:\x\pythonw.exe" -m wordink.desktop.win.app'], daemon_state=lambda: None,
        self_elevated=lambda: False, modules=lambda n: True,
    )
    base.update(over)
    return Probes(**base)


def by(checks):
    return {c.name: c for c in checks}


def test_a_healthy_machine_has_no_failures():
    checks = run_checks(cfg(), {}, good())
    assert exit_code(checks) == 0
    c = by(checks)
    assert c["credentials"].status == "ok" and "Credential Manager" in c["credentials"].detail
    assert c["microphone"].detail == "default input: Microphone (USB)"
    assert c["hotkeys"].status == "ok" and c["daemon"].status == "warn" and c["scheduled task"].status == "ok"
    assert c["legacy app"].status == "ok" and "clipboard fallback" in c["elevation"].detail


def test_the_key_is_never_printed_only_whether_it_exists():
    checks = run_checks(cfg(), {"GROQ_API_KEY": KEY}, good(stored_key=lambda: False))
    text = render(checks) + to_json(checks)
    assert KEY not in text
    assert by(checks)["credentials"].detail == "GROQ_API_KEY is set in the environment"


def test_a_missing_key_fails_with_the_fix():
    c = by(run_checks(cfg(), {}, good(stored_key=lambda: False)))["credentials"]
    assert c.status == "fail" and "set-key" in c.fix


def test_gateway_mode_checks_the_device_token_instead():
    ok = by(run_checks(cfg(), {"WORDINK_GATEWAY_URL": "https://g", "WORDINK_DEVICE_TOKEN": "t"}, good(stored_key=lambda: False)))
    assert ok["credentials"].status == "ok"
    bad = by(run_checks(cfg(), {"WORDINK_GATEWAY_URL": "https://g"}, good()))
    assert bad["credentials"].status == "fail"


def test_no_microphone_points_at_privacy_settings():
    c = by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(input_device=lambda: None)))["microphone"]
    assert c.status == "fail" and "Privacy" in c.fix


def test_a_hotkey_clash_fails_and_names_the_chord():
    c = by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(hotkey_clashes=lambda: ["Alt+Ctrl+D"])))["hotkeys"]
    assert c.status == "fail" and "Alt+Ctrl+D" in c.detail and "AutoHotkey" in c.fix


def test_with_the_daemon_running_the_hotkeys_are_not_probed():
    called = []
    p = good(daemon_state=lambda: "idle", hotkey_clashes=lambda: called.append(1) or ["x"])
    c = by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, p))
    assert not called and c["daemon"].detail == "running, state idle" and "not probed" in c["hotkeys"].detail


def test_bad_hotkeys_in_config_are_a_failure():
    bad = DictateConfig(runtime_dir=Path("r"), vocab_path=Path("v"), failed_dir=Path("f"), hold_bind="Win+D", toggle_bind="Ctrl+Alt+S")
    c = by(run_checks(bad, {"GROQ_API_KEY": "x"}, good()))["hotkeys"]
    assert c.status == "fail" and "Win key" in c.detail


def test_task_states():
    assert by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(task_state=lambda: None)))["scheduled task"].status == "warn"
    d = by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(task_state=lambda: "Disabled")))["scheduled task"]
    assert d.status == "warn" and "-Switch" in d.fix


def test_a_running_legacy_app_is_flagged_but_our_own_process_is_not():
    p = good(process_command_lines=lambda: [r'"C:\g\pythonw.exe" -m wordink.main', "python -m something_else"])
    assert by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, p))["legacy app"].status == "warn"


def test_missing_packages_are_reported_with_the_extra():
    checks = by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(modules=lambda n: n != "pystray")))
    assert checks["pystray package"].status == "fail" and "windows' extra" in checks["pystray package"].fix
    assert checks["keyring package"].status == "ok"


def test_a_probe_that_raises_becomes_a_warning_not_a_crash():
    def boom():
        raise OSError(f"cannot read {KEY}")

    checks = run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(task_state=boom, input_device=boom, process_command_lines=boom, self_elevated=boom))
    assert exit_code(checks) == 0 or all(c.status != "fail" or c.name == "microphone" for c in checks)
    text = render(checks)
    assert KEY not in text  # only the exception type is shown
    assert "OSError" in text


def test_the_default_probes_fail_soft():
    checks = run_checks(cfg(), {"GROQ_API_KEY": "x"}, Probes(modules=lambda n: True))
    assert checks and all(c.status in ("ok", "warn", "fail") for c in checks)
    json.loads(to_json(checks))
