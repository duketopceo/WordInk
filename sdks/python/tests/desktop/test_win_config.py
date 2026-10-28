"""dictate.toml on the Windows layout (plan W2c)."""

from __future__ import annotations

from pathlib import PureWindowsPath

import pytest

from wordink.client import WordInkError
from wordink.desktop.config import load_config

ENV = {
    "USERPROFILE": r"C:\Users\luke",
    "APPDATA": r"C:\Users\luke\AppData\Roaming",
    "LOCALAPPDATA": r"C:\Users\luke\AppData\Local",
}


def load(tmp_path, body: str | None = None):
    path = tmp_path / "dictate.toml"
    if body is not None:
        path.write_text(body, encoding="utf-8")
    return load_config(path, ENV, os_name="windows")


def test_defaults_use_windows_hotkeys_and_appdata_paths(tmp_path):
    cfg = load(tmp_path)
    assert (cfg.hold_bind, cfg.toggle_bind) == ("Ctrl+Alt+D", "Ctrl+Alt+S")
    assert PureWindowsPath(str(cfg.vocab_path)) == PureWindowsPath(r"C:\Users\luke\AppData\Roaming\wordink\vocabulary.txt")
    assert PureWindowsPath(str(cfg.runtime_dir)) == PureWindowsPath(r"C:\Users\luke\AppData\Local\wordink\run")
    assert PureWindowsPath(str(cfg.failed_dir)) == PureWindowsPath(r"C:\Users\luke\AppData\Local\wordink\failed")


def test_the_default_config_path_comes_from_appdata():
    cfg = load_config(None, ENV, os_name="windows")  # no file at that path on this machine: defaults
    assert cfg.hold_bind == "Ctrl+Alt+D"


def test_a_file_overrides_the_hotkeys_and_shared_settings(tmp_path):
    cfg = load(tmp_path, '[hotkey]\nhold = "Ctrl+Shift+F9"\ntoggle = "Alt+F10"\n[cleanup]\ntone = "email"\n[record]\nmax_seconds = 30\n')
    assert (cfg.hold_bind, cfg.toggle_bind, cfg.tone, cfg.max_seconds) == ("Ctrl+Shift+F9", "Alt+F10", "email", 30.0)


@pytest.mark.parametrize("hold", ["Win+D", "SUPER ALT, D", "D", "Ctrl+Alt+Delete"])
def test_hyprland_syntax_and_reserved_chords_are_rejected_on_windows(tmp_path, hold):
    with pytest.raises(WordInkError):
        load(tmp_path, f'[hotkey]\nhold = "{hold}"\n')


def test_hold_equal_to_toggle_is_rejected(tmp_path):
    with pytest.raises(WordInkError, match="different"):
        load(tmp_path, '[hotkey]\nhold = "Ctrl+Alt+D"\ntoggle = "Alt+Ctrl+D"\n')


def test_unknown_keys_still_fail_loudly(tmp_path):
    with pytest.raises(WordInkError, match="unknown key"):
        load(tmp_path, "[hotkey]\nbogus = 1\n")


def test_linux_loading_is_unchanged_and_rejects_windows_syntax(tmp_path):
    p = tmp_path / "d.toml"
    p.write_text('[hotkey]\nhold = "Ctrl+Alt+D"\n')
    with pytest.raises(WordInkError):
        load_config(p, {"HOME": str(tmp_path)})
