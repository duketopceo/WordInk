"""macOS paths, dictate.toml and the CLI edges (plan M1)."""

from __future__ import annotations

from pathlib import PurePosixPath

import pytest

from wordink.client import WordInkError
from wordink.desktop.cli import main
from wordink.desktop.config import load_config
from wordink.desktop.platform import MAC_SOCKET_MAX, app_paths, current_os

ENV = {"HOME": "/Users/luke", "TMPDIR": "/var/folders/ab/cdef0123/T/"}


def load(tmp_path, body: str | None = None, env=ENV):
    path = tmp_path / "dictate.toml"
    if body is not None:
        path.write_text(body, encoding="utf-8")
    return load_config(path, env, os_name="macos")


def test_darwin_is_macos():
    assert current_os("darwin") == "macos"


def test_layout_follows_apples_folders():
    p = app_paths("macos", ENV, uid=501)
    assert p.config_file == PurePosixPath("/Users/luke/Library/Application Support/wordink/dictate.toml")
    assert p.vocab_file == PurePosixPath("/Users/luke/Library/Application Support/wordink/vocabulary.txt")
    assert p.failed_dir == PurePosixPath("/Users/luke/Library/Caches/wordink/failed")
    assert p.runtime_dir == PurePosixPath("/var/folders/ab/cdef0123/T/wordink")


def test_no_tmpdir_uses_a_per_user_tmp_folder():
    assert app_paths("macos", {"HOME": "/Users/luke"}, uid=501).runtime_dir == PurePosixPath("/tmp/wordink-501")


def test_a_long_tmpdir_falls_back_so_the_socket_path_fits():
    env = {"HOME": "/Users/luke", "TMPDIR": "/private/var/folders/" + "x" * 90 + "/T/"}
    p = app_paths("macos", env, uid=501)
    assert p.runtime_dir == PurePosixPath("/tmp/wordink-501")
    assert len(str(p.runtime_dir / "dictd.sock").encode()) <= MAC_SOCKET_MAX


def test_the_socket_path_fits_for_a_typical_tmpdir():
    p = app_paths("macos", ENV, uid=501)
    assert len(str(p.runtime_dir / "dictd.sock").encode()) <= MAC_SOCKET_MAX


def test_without_home_it_says_so():
    with pytest.raises(WordInkError, match="HOME"):
        app_paths("macos", {})


def test_defaults_use_mac_hotkeys_and_library_paths(tmp_path):
    cfg = load(tmp_path)
    assert (cfg.hold_bind, cfg.toggle_bind) == ("Ctrl+Option+D", "Ctrl+Option+S")
    assert str(cfg.vocab_path) == "/Users/luke/Library/Application Support/wordink/vocabulary.txt"
    assert str(cfg.failed_dir) == "/Users/luke/Library/Caches/wordink/failed"
    assert cfg.socket_path.name == "dictd.sock"


def test_a_file_overrides_the_hotkeys_and_shared_settings(tmp_path):
    cfg = load(tmp_path, '[hotkey]\nhold = "Cmd+Shift+F9"\ntoggle = "Alt+F10"\n[cleanup]\ntone = "email"\n')
    assert (cfg.hold_bind, cfg.toggle_bind, cfg.tone) == ("Cmd+Shift+F9", "Alt+F10", "email")


@pytest.mark.parametrize(
    ("hold", "message"),
    [
        ("Cmd+Space", "Spotlight"),
        ("Cmd+Q", "Quit"),
        ("Ctrl+Cmd+Q", "Lock Screen"),
        ("Cmd+Option+Esc", "Force Quit"),
        ("Cmd+Shift+4", "screenshots"),
        ("Ctrl+Space", "input source"),
        ("D", "must look like"),
        ("Shift+D", "Shift alone"),
        ("SUPER ALT, D", "must look like"),
        ("Win+D", "not a Mac key"),
        ("Fn+D", "Fn/Globe"),
        ("Ctrl+Option", "normal key"),
        ("Ctrl+Option+Nope", "unknown key"),
    ],
)
def test_reserved_and_malformed_chords_are_rejected(tmp_path, hold, message):
    with pytest.raises(WordInkError, match=message):
        load(tmp_path, f'[hotkey]\nhold = "{hold}"\n')


def test_alt_and_option_are_the_same_key(tmp_path):
    with pytest.raises(WordInkError, match="different"):
        load(tmp_path, '[hotkey]\nhold = "Ctrl+Option+D"\ntoggle = "Alt+Ctrl+D"\n')


def test_modifier_order_is_normalized(tmp_path):
    cfg = load(tmp_path, '[hotkey]\nhold = "Shift+Option+Ctrl+Cmd+F5"\n')
    from wordink.desktop.mac.hotkey import parse_chord

    assert parse_chord(cfg.hold_bind).text == "Cmd+Ctrl+Option+Shift+F5"
    assert parse_chord("ctrl+opt+d").key == 0x02  # kVK_ANSI_D


def test_key_codes_for_letters_digits_and_function_keys():
    from wordink.desktop.mac.hotkey import parse_chord

    assert parse_chord("Ctrl+Option+S").key == 0x01
    assert parse_chord("Ctrl+Option+5").key == 0x17
    assert parse_chord("Ctrl+Option+F13").key == 0x69
    assert parse_chord("Ctrl+Option+Space").key == 0x31


def test_unknown_keys_still_fail_loudly(tmp_path):
    with pytest.raises(WordInkError, match="unknown key"):
        load(tmp_path, "[hotkey]\nbogus = 1\n")


# --- CLI -----------------------------------------------------------------------------------------


@pytest.fixture
def macenv(monkeypatch, tmp_path):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("TMPDIR", str(tmp_path) + "/")
    return tmp_path


def test_mac_binds_print_the_chords(macenv, capsys):
    assert main(["binds"], os_name="macos") == 0
    out = capsys.readouterr().out
    assert "Ctrl+Option+D" in out and "Ctrl+Option+S" in out and "bind" not in out


def test_mac_waybar_is_refused_politely(macenv, capsys):
    assert main(["waybar"], os_name="macos") == 1
    assert "menu bar" in capsys.readouterr().err


def test_mac_run_says_it_is_not_wired_yet(macenv, capsys):
    assert main(["run"], os_name="macos") == 1
    assert "M6" in capsys.readouterr().err


def test_mac_set_key_stores_in_the_keychain_without_echo(macenv, capsys, monkeypatch):
    from wordink.desktop.mac import secrets

    class Backend:
        data: dict = {}

        def get_password(self, s, u):
            return self.data.get((s, u))

        def set_password(self, s, u, p):
            self.data[(s, u)] = p

        def delete_password(self, s, u):
            del self.data[(s, u)]

    backend = Backend()
    real = secrets.KeyStore
    monkeypatch.setattr(secrets, "KeyStore", lambda: real(backend))
    monkeypatch.setattr(secrets.getpass, "getpass", lambda _p: "gsk_mac_secret_9")
    assert main(["set-key"], os_name="macos") == 0
    out = capsys.readouterr()
    assert "macOS Keychain" in out.out
    assert "gsk_mac_secret_9" not in out.out + out.err
    assert backend.data[(secrets.SERVICE, secrets.KEY_NAME)] == "gsk_mac_secret_9"


def test_return_and_keypad_enter_are_different_keys():
    from wordink.desktop.mac.hotkey import parse_chord

    assert parse_chord("Ctrl+Option+Return").key == 0x24
    assert parse_chord("Ctrl+Option+Enter").key == 0x4C
