"""examples/macos-dictation/install.sh, run for real against fake system tools in a throwaway HOME.

Nothing here touches the real machine: HOME and PATH point into tmp_path, and the "system" commands
(uname, uv, launchctl, osascript, open, pgrep, wordink-dictd) are scripts that only log their argv.
Nothing in this file has run against a real Mac; that is plan step M9.
"""

from __future__ import annotations

import os
import plistlib
import shutil
import stat
import subprocess
from pathlib import Path

import pytest

EXAMPLES = Path(__file__).resolve().parents[4] / "examples" / "macos-dictation"
SCRIPT = EXAMPLES / "install.sh"
TEMPLATE = EXAMPLES / "io.wordink.dictd.plist.tmpl"
CANARY = "gsk_canary_not_a_real_key_0123456789"

FAKES = {
    "uname": 'if [ "$1" = "-s" ]; then echo Darwin; else echo Darwin; fi',
    # `uv tool install` mimics uv: a tool venv with the entry point, and a symlink shim in the bin dir.
    "uv": r'''
echo "uv $*" >> "$FAKE/log"
case "$1 $2" in
  "tool dir") echo "$FAKE/tools" ;;
  "tool install")
    mkdir -p "$FAKE/tools/wordink/bin" "$HOME/.local/bin"
    printf '#!/bin/sh\nexit 0\n' > "$FAKE/tools/wordink/bin/wordink-dictd-mac"; chmod +x "$FAKE/tools/wordink/bin/wordink-dictd-mac"
    ln -sf "$FAKE/tools/wordink/bin/wordink-dictd-mac" "$HOME/.local/bin/wordink-dictd-mac" ;;
  "tool uninstall") rm -f "$HOME/.local/bin/wordink-dictd-mac" ;;
esac
exit 0''',
    "launchctl": r'''
echo "launchctl $*" >> "$FAKE/log"
case "$1" in
  print) [ -f "$FAKE/loaded" ] ;;
  bootstrap) : > "$FAKE/loaded" ;;
  bootout) rm -f "$FAKE/loaded" ;;
esac''',
    "osascript": 'echo "osascript $*" >> "$FAKE/log"',
    "open": 'echo "open $*" >> "$FAKE/log"',
    "pgrep": r'''[ -f "$FAKE/running" ] && [ "$2" = "$(cat "$FAKE/running")" ]''',
    "wordink-dictd": 'echo "wordink-dictd $*" >> "$FAKE/log"; echo "doctor ok"',
}


class Env:
    def __init__(self, root: Path, skip: tuple[str, ...] = ()) -> None:
        self.root = root
        self.home = root / "home"
        self.fake = root / "fake"
        self.bin = root / "bin"
        for d in (self.home / ".local" / "bin", self.fake, self.bin):
            d.mkdir(parents=True, exist_ok=True)
        for name, body in FAKES.items():
            if name in skip:
                continue
            exe = self.bin / ("uv" if name == "uv" else name)
            exe.write_text(f"#!/bin/sh\n{body}\n")
            exe.chmod(exe.stat().st_mode | stat.S_IXUSR)
        # wordink-dictd lives in the user's bin dir (uv puts it there), not on the fake PATH
        dictd = self.bin / "wordink-dictd"
        if dictd.exists():
            shutil.move(str(dictd), str(self.home / ".local" / "bin" / "wordink-dictd"))
        (self.fake / "log").write_text("")

    def run(self, *args: str, extra: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        env = {
            "HOME": str(self.home),
            "PATH": f"{self.bin}:/usr/bin:/bin",
            "FAKE": str(self.fake),
            "GROQ_API_KEY": CANARY,  # must never end up anywhere
            **(extra or {}),
        }
        return subprocess.run(["bash", str(SCRIPT), *args], env=env, capture_output=True, text=True, timeout=60)

    @property
    def log(self) -> list[str]:
        return (self.fake / "log").read_text().splitlines()

    def snapshot(self) -> dict[str, str]:
        out: dict[str, str] = {}
        for p in sorted(self.home.rglob("*")):
            if p.is_symlink():
                out[str(p.relative_to(self.home))] = f"-> {os.readlink(p)}"
            elif p.is_file():
                out[str(p.relative_to(self.home))] = p.read_text()
        return out

    def path(self, rel: str) -> Path:
        return self.home / rel


@pytest.fixture
def env(tmp_path):
    return Env(tmp_path)


PLIST = "Library/LaunchAgents/io.wordink.dictd.plist"


def test_install_writes_launcher_plist_and_loads_the_agent(env):
    r = env.run()
    assert r.returncode == 0, r.stderr + r.stdout
    launcher = env.path(".local/bin/wordink-dictd-mac")
    assert launcher.is_file() and not launcher.is_symlink()
    assert stat.S_IMODE(launcher.stat().st_mode) == 0o755
    assert launcher.read_text().startswith("#!/bin/sh\n") and f"{env.fake}/tools/wordink/bin/wordink-dictd-mac" in launcher.read_text()
    assert any(l.startswith("uv tool install --force wordink[macos] @ ") and "sdks/python" in l for l in env.log)
    assert [l for l in env.log if l.startswith("launchctl bootstrap")] == [f"launchctl bootstrap gui/{os.getuid()} {env.path(PLIST)}"]
    assert "doctor ok" in r.stdout and "wordink-dictd doctor" in env.log
    assert (env.fake / "loaded").exists()


def test_the_plist_is_valid_and_has_the_launchd_settings_the_plan_asks_for(env):
    env.run()
    data = plistlib.loads(env.path(PLIST).read_bytes())
    assert data["Label"] == "io.wordink.dictd"
    assert data["ProgramArguments"] == [str(env.path(".local/bin/wordink-dictd-mac"))]
    assert data["RunAtLoad"] is True
    assert data["KeepAlive"] == {"SuccessfulExit": False}
    assert data["ThrottleInterval"] >= 10
    assert data["ProcessType"] == "Interactive"
    logs = str(env.path("Library/Logs/wordink"))
    assert data["StandardErrorPath"].startswith(logs) and data["StandardOutPath"].startswith(logs)
    assert env.path("Library/Logs/wordink").is_dir()


def test_the_template_has_only_the_two_placeholders_and_renders_to_valid_xml():
    text = TEMPLATE.read_text()
    assert "__LAUNCHER__" in text and "__LOG_DIR__" in text
    rendered = text.replace("__LAUNCHER__", "/h/.local/bin/wordink-dictd-mac").replace("__LOG_DIR__", "/h/Library/Logs/wordink")
    assert "__" not in rendered
    plistlib.loads(rendered.encode())


def test_no_key_ever_lands_in_a_file_or_in_output(env):
    r = env.run()
    assert CANARY not in r.stdout + r.stderr
    assert CANARY not in "\n".join(env.snapshot().values())
    assert CANARY not in "\n".join(env.log)
    assert not env.path("Library/Application Support/wordink").exists() or not any(env.path("Library/Application Support/wordink").iterdir())


def test_rerunning_changes_nothing_and_does_not_reload(env):
    env.run()
    before, log_len = env.snapshot(), len(env.log)
    again = env.run()
    assert again.returncode == 0
    assert env.snapshot() == before
    assert not any(l.startswith(("launchctl bootstrap", "launchctl bootout")) for l in env.log[log_len:])
    assert "already loaded and up to date" in again.stdout


def test_a_changed_plist_is_replaced_and_the_agent_restarted(env):
    env.run()
    env.path(PLIST).write_text("stale")
    log_len = len(env.log)
    env.run()
    assert plistlib.loads(env.path(PLIST).read_bytes())["Label"] == "io.wordink.dictd"
    cmds = [l for l in env.log[log_len:] if l.startswith("launchctl b")]
    assert cmds == [f"launchctl bootout gui/{os.getuid()}/io.wordink.dictd", f"launchctl bootstrap gui/{os.getuid()} {env.path(PLIST)}"]


def test_dry_run_changes_nothing_and_says_what_it_would_do(env):
    before = env.snapshot()
    r = env.run("--dry-run", "--switch", "Superwhisper", extra={})
    assert r.returncode == 0, r.stderr + r.stdout
    assert env.snapshot() == before
    assert "[dry-run] uv tool install --force" in r.stdout
    assert f"[dry-run] write {env.path(PLIST)}" in r.stdout
    assert "[dry-run] launchctl bootstrap" in r.stdout
    assert not any(l.startswith(("uv tool install", "launchctl bootstrap", "launchctl bootout", "osascript")) for l in env.log)
    assert not (env.fake / "loaded").exists()


def test_no_start_writes_the_files_but_does_not_load_the_agent(env):
    r = env.run("--no-start")
    assert r.returncode == 0
    assert env.path(PLIST).exists()
    assert not any(l.startswith("launchctl bootstrap") for l in env.log)
    assert "Not loading the LaunchAgent" in r.stdout


def test_a_missing_uv_fails_with_the_install_hint_and_changes_nothing(tmp_path):
    e = Env(tmp_path, skip=("uv",))
    before = e.snapshot()
    r = e.run()
    assert r.returncode == 1
    assert "uv is required" in r.stderr and "astral.sh/uv/install.sh" in r.stderr
    assert e.snapshot() == before and e.log == []


def test_a_non_mac_is_refused_before_anything_happens(tmp_path):
    e = Env(tmp_path, skip=("uname",))  # the real uname says Linux here
    if subprocess.run(["uname", "-s"], capture_output=True, text=True).stdout.strip() == "Darwin":
        pytest.skip("running on a Mac")
    before = e.snapshot()
    r = e.run()
    assert r.returncode == 1 and "for macOS" in r.stderr
    assert e.snapshot() == before and e.log == []


def test_without_switch_no_other_app_is_touched(env):
    (env.fake / "running").write_text("Superwhisper")
    env.run()
    assert not any(l.startswith(("osascript", "open")) for l in env.log)
    assert not env.path(".local/bin/wordink-use-previous").exists()


def test_switch_quits_the_named_app_and_writes_a_one_command_rollback(env):
    (env.fake / "running").write_text("Superwhisper")
    r = env.run("--switch", "Superwhisper")
    assert r.returncode == 0, r.stderr + r.stdout
    assert 'osascript -e tell application "Superwhisper" to quit' in env.log
    state = env.path("Library/Application Support/wordink/switch-before").read_text()
    assert "app=Superwhisper" in state and "running=yes" in state
    rollback = env.path(".local/bin/wordink-use-previous")
    assert stat.S_IMODE(rollback.stat().st_mode) == 0o755

    log_len = len(env.log)
    done = subprocess.run(
        [str(rollback)], env={"PATH": f"{env.bin}:/usr/bin:/bin", "FAKE": str(env.fake)}, capture_output=True, text=True
    )
    assert done.returncode == 0, done.stderr
    assert env.log[log_len:] == [f"launchctl bootout gui/{os.getuid()}/io.wordink.dictd", "open -a Superwhisper"]
    assert not env.path("Library/Application Support/wordink/switch-before").exists()
    assert "Superwhisper is back" in done.stdout


def test_switch_does_not_reopen_an_app_that_was_not_running(env):
    r = env.run("--switch", "Superwhisper")
    assert r.returncode == 0
    assert not any(l.startswith("osascript") for l in env.log)
    rollback = env.path(".local/bin/wordink-use-previous")
    log_len = len(env.log)
    subprocess.run([str(rollback)], env={"PATH": f"{env.bin}:/usr/bin:/bin", "FAKE": str(env.fake)}, check=True, capture_output=True)
    assert not any(l.startswith("open") for l in env.log[log_len:])


def test_switch_names_with_shell_metacharacters_stay_inert(env):
    (env.fake / "running").write_text("Dictate Pro's $(touch pwned)")
    r = env.run("--switch", "Dictate Pro's $(touch pwned)")
    assert r.returncode == 0, r.stderr + r.stdout
    rollback = env.path(".local/bin/wordink-use-previous")
    subprocess.run([str(rollback)], env={"PATH": f"{env.bin}:/usr/bin:/bin", "FAKE": str(env.fake)}, check=True, capture_output=True, cwd=env.root)
    assert "open -a Dictate Pro's $(touch pwned)" in env.log
    assert not (env.root / "pwned").exists()


@pytest.mark.parametrize("name", ['Bad"Name', "Bad\\Name"])
def test_switch_rejects_names_that_could_break_the_quit_command(env, name):
    r = env.run("--switch", name)
    assert r.returncode == 2 and "may not contain" in r.stderr
    assert env.log == []


def test_switch_needs_a_name_and_a_started_agent(env):
    assert env.run("--switch").returncode == 2
    r = env.run("--switch", "X", "--no-start")
    assert r.returncode == 2 and "--no-start" in r.stderr


def test_a_second_switch_run_keeps_the_original_record(env):
    (env.fake / "running").write_text("Superwhisper")
    env.run("--switch", "Superwhisper")
    state = env.path("Library/Application Support/wordink/switch-before")
    original = state.read_text()
    (env.fake / "running").unlink()  # it is quit now, so a re-run would otherwise record "no"
    env.run("--switch", "Superwhisper")
    assert state.read_text() == original


def test_uninstall_removes_what_the_installer_added_and_keeps_key_and_vocabulary(env):
    support = env.path("Library/Application Support/wordink")
    support.mkdir(parents=True)
    (support / "vocabulary.txt").write_text("WordInk\n")
    (support / "dictate.toml").write_text("[hotkey]\n")
    (env.fake / "running").write_text("Superwhisper")
    env.run("--switch", "Superwhisper")
    r = env.run("--uninstall")
    assert r.returncode == 0, r.stderr + r.stdout
    assert not env.path(PLIST).exists()
    assert not env.path(".local/bin/wordink-dictd-mac").exists()
    assert not env.path(".local/bin/wordink-use-previous").exists()
    assert not (support / "switch-before").exists()
    assert (support / "vocabulary.txt").read_text() == "WordInk\n"
    assert (support / "dictate.toml").read_text() == "[hotkey]\n"
    assert not (env.fake / "loaded").exists()
    assert "uv tool uninstall wordink" in env.log
    assert not any(l.startswith("security") or "keychain" in l.lower() for l in env.log), "the Keychain is never touched"


def test_uninstall_when_nothing_is_installed_is_a_no_op(env):
    r = env.run("--uninstall")
    assert r.returncode == 0
    assert not any(l.startswith("launchctl bootout") for l in env.log)


def test_uninstall_dry_run_touches_nothing(env):
    env.run()
    before, log_len = env.snapshot(), len(env.log)
    r = env.run("--uninstall", "--dry-run")
    assert r.returncode == 0 and env.snapshot() == before
    assert "[dry-run] launchctl bootout" in r.stdout
    assert not any(l.startswith(("launchctl bootout", "uv tool uninstall")) for l in env.log[log_len:])
    assert (env.fake / "loaded").exists()


def test_a_home_with_xml_special_characters_is_refused(tmp_path):
    e = Env(tmp_path)
    r = e.run(extra={"HOME": str(tmp_path / "a&b")})
    assert r.returncode == 1 and "plist" in r.stderr


def test_unknown_option_is_rejected(env):
    r = env.run("--nope")
    assert r.returncode == 2 and "unknown option" in r.stderr


def test_script_is_executable_and_has_a_shebang():
    assert os.access(SCRIPT, os.X_OK)
    assert SCRIPT.read_text().startswith("#!/usr/bin/env bash\n")


def test_script_has_no_bash_4_only_features():
    # macOS ships bash 3.2: no mapfile/readarray, associative arrays, ${var,,}, ${var^^} or &>>
    text = SCRIPT.read_text()
    for banned in ("mapfile", "readarray", "declare -A", ",,}", "^^}", "&>>"):
        assert banned not in text, banned


@pytest.mark.skipif(shutil.which("shellcheck") is None, reason="shellcheck is not installed")
def test_shellcheck_is_clean():
    done = subprocess.run(["shellcheck", str(SCRIPT)], capture_output=True, text=True)
    assert done.returncode == 0, done.stdout
