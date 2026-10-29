"""macOS doctor against fake probes (plan M6)."""

from __future__ import annotations

from pathlib import Path

from wordink.desktop.config import DictateConfig
from wordink.desktop.doctor import exit_code, render, to_json
from wordink.desktop.mac.doctor import Probes, run_checks

KEY = "gsk_do_not_leak_me"


def cfg(**kw):
    return DictateConfig(runtime_dir=Path("r"), vocab_path=Path("v"), failed_dir=Path("f"), hold_bind="Ctrl+Option+D", toggle_bind="Ctrl+Option+S", **kw)


def good(**over):
    base = dict(
        mac_version=lambda: ("15.1", "arm64"), stored_key=lambda: True, input_device=lambda: "MacBook Pro Microphone",
        microphone_access=lambda: "granted", accessibility_trusted=lambda: True, secure_input=lambda: False,
        hotkey_clashes=lambda: [], launch_agent_state=lambda: "running", daemon_state=lambda: None,
        permission_holder=lambda: "/Users/x/.local/share/uv/python/bin/python3.11", modules=lambda n: True,
    )  # fmt: skip
    base.update(over)
    return Probes(**base)


def by(checks):
    return {c.name: c for c in checks}


def test_a_healthy_mac_has_no_failures():
    checks = run_checks(cfg(), {}, good())
    assert exit_code(checks) == 0
    c = by(checks)
    assert c["macOS"].detail == "15.1 (arm64)" and "Keychain" in c["credentials"].detail
    assert c["microphone access"].status == "ok" and c["accessibility"].status == "ok"
    assert c["hotkeys"].status == "ok" and c["daemon"].status == "warn" and c["launch agent"].status == "ok"
    assert c["input monitoring"].status == "ok" and "not needed" in c["input monitoring"].detail


def test_the_key_is_never_printed_only_whether_it_exists():
    checks = run_checks(cfg(), {"GROQ_API_KEY": KEY}, good(stored_key=lambda: False))
    assert KEY not in render(checks) + to_json(checks)
    assert by(checks)["credentials"].detail == "GROQ_API_KEY is set in the environment"


def test_a_missing_key_fails_with_the_fix():
    c = by(run_checks(cfg(), {}, good(stored_key=lambda: False)))["credentials"]
    assert c.status == "fail" and "set-key" in c.fix


def test_gateway_mode_checks_the_device_token_instead():
    assert by(run_checks(cfg(), {"WORDINK_GATEWAY_URL": "g", "WORDINK_DEVICE_TOKEN": "t"}, good(stored_key=lambda: False)))["credentials"].status == "ok"
    assert by(run_checks(cfg(), {"WORDINK_GATEWAY_URL": "g"}, good()))["credentials"].status == "fail"


def test_permissions_name_the_program_that_needs_them():
    c = by(run_checks(cfg(), {}, good(accessibility_trusted=lambda: False, microphone_access=lambda: "denied")))
    assert c["accessibility"].status == "fail" and "python3.11" in c["accessibility"].fix
    assert c["microphone access"].status == "fail" and "Microphone" in c["microphone access"].fix
    assert c["permission holder"].detail.startswith("grants go to ")


def test_microphone_access_unknown_or_not_asked_is_only_a_warning():
    assert by(run_checks(cfg(), {}, good(microphone_access=lambda: None)))["microphone access"].status == "warn"
    assert by(run_checks(cfg(), {}, good(microphone_access=lambda: "undetermined")))["microphone access"].status == "warn"


def test_no_input_device_fails():
    c = by(run_checks(cfg(), {}, good(input_device=lambda: None)))["microphone"]
    assert c.status == "fail" and "System Settings" in c.fix


def test_secure_input_is_a_warning():
    assert by(run_checks(cfg(), {}, good(secure_input=lambda: True)))["secure input"].status == "warn"


def test_a_hotkey_clash_fails_and_a_running_daemon_skips_the_probe():
    assert by(run_checks(cfg(), {}, good(hotkey_clashes=lambda: ["Ctrl+Option+D"])))["hotkeys"].status == "fail"

    def boom():
        raise AssertionError("must not probe while the daemon owns the chords")

    c = by(run_checks(cfg(), {}, good(daemon_state=lambda: "idle", hotkey_clashes=boom)))
    assert c["hotkeys"].detail.endswith("(daemon running, not probed)") and c["daemon"].status == "ok"


def test_bad_hotkeys_in_config_are_a_failure():
    bad = DictateConfig(runtime_dir=Path("r"), vocab_path=Path("v"), failed_dir=Path("f"), hold_bind="Cmd+Space", toggle_bind="Ctrl+Option+S")
    assert by(run_checks(bad, {}, good()))["hotkeys"].status == "fail"


def test_old_macos_and_missing_launch_agent_warn():
    c = by(run_checks(cfg(), {}, good(mac_version=lambda: ("12.7", "x86_64"), launch_agent_state=lambda: None)))
    assert c["macOS"].status == "warn" and c["launch agent"].status == "warn"


def test_missing_packages_are_reported_with_the_extra():
    c = by(run_checks(cfg(), {}, good(modules=lambda n: n != "rumps")))
    assert c["rumps package"].status == "fail" and "'macos' extra" in c["rumps package"].fix


def test_a_probe_that_raises_becomes_a_warning_not_a_crash():
    def boom():
        raise OSError("no")

    c = by(run_checks(cfg(), {"GROQ_API_KEY": "x"}, good(accessibility_trusted=boom, secure_input=boom, launch_agent_state=boom)))
    assert c["accessibility"].status == "warn" and c["secure input"].status == "warn" and c["launch agent"].status == "warn"


def test_the_default_probes_fail_soft():
    checks = run_checks(cfg(), {"GROQ_API_KEY": "x"}, Probes(modules=lambda n: True))
    assert exit_code(checks) in (0, 1) and all(c.status in ("ok", "warn", "fail") for c in checks)


def test_the_doctor_command_runs_on_macos(monkeypatch, tmp_path, capsys):
    from wordink.desktop.cli import main
    from wordink.desktop.mac import doctor as md

    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("TMPDIR", str(tmp_path) + "/")
    monkeypatch.setattr(md, "real_probes", lambda cfg: good())
    assert main(["doctor", "--json"], os_name="macos") == 0
    assert '"accessibility"' in capsys.readouterr().out
