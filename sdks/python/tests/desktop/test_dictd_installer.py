"""examples/omarchy-dictation/install.sh, run for real against fake system tools in a throwaway HOME.

Nothing here touches the real machine: HOME, XDG paths and PATH all point into tmp_path, and the
"system" commands (systemctl, uv, hyprctl, sudo, wordink-dictd) are scripts that only log their argv.
"""

from __future__ import annotations

import json
import os
import stat
import subprocess
from pathlib import Path

import pytest

EXAMPLES = Path(__file__).resolve().parents[4] / "examples" / "omarchy-dictation"
SCRIPT = EXAMPLES / "install.sh"

FAKES = {
    "systemctl": r'''
echo "systemctl $*" >> "$FAKE/log"
case "$*" in
  *list-unit-files*) [ -f "$FAKE/no_voxtype" ] || echo "voxtype.service enabled enabled" ;;
  *is-enabled*) echo enabled ;;
  *is-active*) echo active ;;
esac
exit 0''',
    "uv": 'echo "uv $*" >> "$FAKE/log"',
    "hyprctl": 'echo "hyprctl $*" >> "$FAKE/log"',
    "sudo": 'echo "sudo $*" >> "$FAKE/log"',
    "pw-record": "exit 0",
    "wl-copy": "exit 0",
    "notify-send": "exit 0",
    "wtype": "exit 0",
    "wordink-dictd": r'''
case "$1" in
  binds) printf '%s\n' "bind = SUPER ALT, D, exec, wordink-dictd trigger start" "bindr = SUPER ALT, D, exec, wordink-dictd trigger stop" "bind = SUPER ALT, S, exec, wordink-dictd trigger toggle" ;;
  doctor) if [ "$2" = "--json" ]; then cat "$FAKE/doctor.json"; else echo "doctor ok"; fi ;;
esac''',
}


class Env:
    def __init__(self, root: Path, skip: tuple[str, ...] = ()) -> None:
        self.root = root
        self.home = root / "home"
        self.fake = root / "fake"
        self.bin = root / "bin"
        for d in (self.home, self.fake, self.bin, self.home / ".config" / "hypr"):
            d.mkdir(parents=True, exist_ok=True)
        for name, body in FAKES.items():
            if name in skip:
                continue
            exe = self.bin / name
            exe.write_text(f"#!/bin/sh\n{body}\n")
            exe.chmod(exe.stat().st_mode | stat.S_IXUSR)
        (self.fake / "doctor.json").write_text("[]")
        (self.fake / "log").write_text("")
        (self.home / ".config" / "hypr" / "hyprland.conf").write_text("# my config\nmonitor=,preferred,auto,1\n")

    def run(self, *args: str, extra: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        env = {
            "HOME": str(self.home),
            "PATH": f"{self.bin}:/usr/bin:/bin",
            "FAKE": str(self.fake),
            "HYPRLAND_INSTANCE_SIGNATURE": "test",
            **(extra or {}),
        }
        return subprocess.run(["bash", str(SCRIPT), *args], env=env, capture_output=True, text=True, timeout=60)

    @property
    def log(self) -> list[str]:
        return (self.fake / "log").read_text().splitlines()

    def snapshot(self) -> dict[str, str]:
        return {str(p.relative_to(self.home)): p.read_text() for p in sorted(self.home.rglob("*")) if p.is_file()}

    def path(self, rel: str) -> Path:
        return self.home / rel


@pytest.fixture
def env(tmp_path):
    return Env(tmp_path)


def test_default_install_writes_files_and_leaves_voxtype_alone(env):
    r = env.run()
    assert r.returncode == 0, r.stderr + r.stdout
    unit = env.path(".config/systemd/user/wordink-dictd.service")
    assert unit.read_text() == (EXAMPLES / "wordink-dictd.service").read_text()
    assert env.path(".config/hypr/wordink-dictd.conf").read_text().startswith("bind = SUPER ALT, D")
    hypr = env.path(".config/hypr/hyprland.conf").read_text()
    assert hypr.startswith("# my config\nmonitor=,preferred,auto,1\n")
    assert f"source = {env.path('.config/hypr/wordink-dictd.conf')}" in hypr
    key_file = env.path(".config/wordink/dictd.env")
    assert stat.S_IMODE(key_file.stat().st_mode) == 0o600 and "GROQ_API_KEY" in key_file.read_text()
    assert any(l.startswith("uv tool install --force") and "sdks/python" in l for l in env.log)
    assert "systemctl --user daemon-reload" in env.log
    # Voxtype is never stopped, disabled, or even queried, and WordInk is not started
    assert not any("voxtype" in l or "disable" in l or "enable" in l for l in env.log)
    assert not any(l.startswith("systemctl --user start") for l in env.log)
    assert not env.path(".local/bin/wordink-use-voxtype").exists()
    assert "Voxtype is untouched" in r.stdout


def test_rerunning_changes_nothing(env):
    env.run()
    before = env.snapshot()
    again = env.run()
    assert again.returncode == 0
    assert env.snapshot() == before
    assert env.path(".config/hypr/hyprland.conf").read_text().count("wordink-dictd (managed") == 1


def test_a_hypr_config_without_a_trailing_newline_is_extended_and_restored_exactly(env):
    conf = env.path(".config/hypr/hyprland.conf")
    conf.write_text("monitor=,preferred,auto,1")  # no final newline
    env.run()
    assert conf.read_text().startswith("monitor=,preferred,auto,1\n# >>> wordink-dictd")
    env.run("--uninstall")
    assert conf.read_text() == "monitor=,preferred,auto,1\n"


def test_an_existing_key_file_is_never_overwritten(env):
    key_file = env.path(".config/wordink/dictd.env")
    key_file.parent.mkdir(parents=True)
    key_file.write_text("GROQ_API_KEY=keep-me\n")
    env.run()
    assert key_file.read_text() == "GROQ_API_KEY=keep-me\n"
    assert stat.S_IMODE(key_file.stat().st_mode) == 0o600


def test_dry_run_changes_nothing_and_says_what_it_would_do(env):
    before = env.snapshot()
    r = env.run("--dry-run", "--switch")
    assert r.returncode == 0, r.stderr + r.stdout
    assert env.snapshot() == before
    assert "[dry-run] uv tool install --force" in r.stdout
    assert "[dry-run] systemctl --user disable --now voxtype.service" in r.stdout
    assert not any(l.startswith(("uv tool", "hyprctl reload")) for l in env.log)
    assert not any("disable" in l or "enable" in l for l in env.log)


def test_a_clashing_bind_aborts_before_any_bind_is_added(env):
    (env.fake / "doctor.json").write_text(
        json.dumps([{"name": "hold bind", "status": "fail", "detail": "SUPER ALT, D is already bound to: exec other", "fix": ""}])
    )
    r = env.run()
    assert r.returncode == 1
    assert "already bound" in r.stdout and "exec other" in r.stdout
    assert not env.path(".config/hypr/wordink-dictd.conf").exists()
    assert "managed" not in env.path(".config/hypr/hyprland.conf").read_text()


def test_switch_stops_voxtype_starts_wordink_and_writes_a_one_command_rollback(env):
    r = env.run("--switch")
    assert r.returncode == 0, r.stderr + r.stdout
    assert "systemctl --user disable --now voxtype.service" in env.log
    assert "systemctl --user enable --now wordink-dictd.service" in env.log
    assert env.log.index("systemctl --user disable --now voxtype.service") < env.log.index("systemctl --user enable --now wordink-dictd.service")
    before = env.path(".local/state/wordink/voxtype-before").read_text()
    assert "unit=voxtype.service" in before and "enabled=enabled" in before and "active=active" in before
    rollback = env.path(".local/bin/wordink-use-voxtype")
    assert stat.S_IMODE(rollback.stat().st_mode) == 0o755
    body = rollback.read_text()
    assert "disable --now wordink-dictd.service" in body and "enable --now voxtype.service" in body

    # running the rollback (against the fake systemctl) really is one command
    before_len = len(env.log)
    done = subprocess.run(
        [str(rollback)], env={"PATH": f"{env.bin}:/usr/bin:/bin", "FAKE": str(env.fake)}, capture_output=True, text=True
    )
    assert done.returncode == 0
    assert env.log[before_len:] == [
        "systemctl --user disable --now wordink-dictd.service",
        "systemctl --user enable --now voxtype.service",
    ]
    assert "Voxtype (voxtype.service) is back" in done.stdout


def test_switch_without_a_voxtype_unit_refuses_and_changes_nothing_else(env):
    (env.fake / "no_voxtype").write_text("")
    r = env.run("--switch")
    assert r.returncode == 1 and "No Voxtype user unit" in r.stdout
    assert not env.path(".local/bin/wordink-use-voxtype").exists()
    assert "systemctl --user enable --now wordink-dictd.service" not in env.log


def test_voxtype_unit_name_can_be_overridden(env):
    r = env.run("--switch", extra={"VOXTYPE_UNIT": "voxtype-daemon.service"})
    assert r.returncode == 0
    assert "systemctl --user disable --now voxtype-daemon.service" in env.log
    assert "enable --now voxtype-daemon.service" in env.path(".local/bin/wordink-use-voxtype").read_text()


def test_uninstall_removes_only_what_the_installer_added(env):
    env.run("--switch")
    r = env.run("--uninstall")
    assert r.returncode == 0, r.stderr + r.stdout
    assert not env.path(".config/systemd/user/wordink-dictd.service").exists()
    assert not env.path(".config/hypr/wordink-dictd.conf").exists()
    assert not env.path(".local/bin/wordink-use-voxtype").exists()
    assert env.path(".config/hypr/hyprland.conf").read_text() == "# my config\nmonitor=,preferred,auto,1\n"
    assert env.path(".config/wordink/dictd.env").exists(), "the user's key file must survive an uninstall"
    assert "uv tool uninstall wordink" in env.log


def test_uninstall_dry_run_touches_nothing(env):
    env.run()
    before, log_len = env.snapshot(), len(env.log)
    r = env.run("--uninstall", "--dry-run")
    assert r.returncode == 0 and env.snapshot() == before
    assert env.log[log_len:] == []


def test_missing_packages_are_listed_not_installed_unless_asked(tmp_path):
    e = Env(tmp_path, skip=("wtype", "wl-copy"))
    r = e.run()
    assert "Missing: wtype wl-clipboard" in r.stdout
    assert not any(l.startswith("sudo") for l in e.log)
    tmp2 = tmp_path / "second"
    tmp2.mkdir()
    e2 = Env(tmp2, skip=("wtype",))
    e2.run("--install-packages")
    assert "sudo pacman -S --needed --noconfirm wtype" in e2.log


def test_unknown_option_is_rejected(env):
    r = env.run("--nope")
    assert r.returncode == 2 and "unknown option" in r.stderr


def test_script_is_executable_and_has_a_shebang():
    assert os.access(SCRIPT, os.X_OK)
    assert SCRIPT.read_text().startswith("#!/usr/bin/env bash\n")
