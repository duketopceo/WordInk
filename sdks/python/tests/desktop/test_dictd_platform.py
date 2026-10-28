"""The platform seams: protocols are structural and the per-OS file layout is a pure function."""

from __future__ import annotations

from pathlib import Path, PureWindowsPath

import pytest

from wordink.client import WordInkError
from wordink.desktop import interfaces
from wordink.desktop.config import default_config_path, default_runtime_dir
from wordink.desktop.platform import app_paths, current_os
from wordink.desktop.transcribe import default_failed_dir
from wordink.desktop.vocab import default_vocab_path

LINUX_ENV = {
    "HOME": "/home/luke",
    "XDG_CONFIG_HOME": "/cfg",
    "XDG_CACHE_HOME": "/cache",
    "XDG_RUNTIME_DIR": "/run/user/1000",
}
WIN_ENV = {
    "USERPROFILE": r"C:\Users\luke",
    "APPDATA": r"C:\Users\luke\AppData\Roaming",
    "LOCALAPPDATA": r"C:\Users\luke\AppData\Local",
}


def test_linux_layout_matches_what_the_daemon_already_uses():
    p = app_paths("linux", LINUX_ENV)
    assert Path(str(p.config_file)) == default_config_path(LINUX_ENV)
    assert Path(str(p.vocab_file)) == default_vocab_path(LINUX_ENV)
    assert Path(str(p.runtime_dir)) == default_runtime_dir(LINUX_ENV)
    assert Path(str(p.failed_dir)) == default_failed_dir(LINUX_ENV)


def test_linux_falls_back_to_home_and_tmp():
    p = app_paths("linux", {"HOME": "/home/luke"}, uid=1234)
    assert str(p.config_file) == "/home/luke/.config/wordink/dictate.toml"
    assert str(p.failed_dir) == "/home/luke/.cache/wordink/failed"
    assert str(p.runtime_dir) == "/tmp/wordink-1234"


def test_windows_layout_uses_appdata_and_localappdata():
    p = app_paths("windows", WIN_ENV)
    assert p.config_file == PureWindowsPath(r"C:\Users\luke\AppData\Roaming\wordink\dictate.toml")
    assert p.vocab_file == PureWindowsPath(r"C:\Users\luke\AppData\Roaming\wordink\vocabulary.txt")
    assert p.runtime_dir == PureWindowsPath(r"C:\Users\luke\AppData\Local\wordink\run")
    assert p.failed_dir == PureWindowsPath(r"C:\Users\luke\AppData\Local\wordink\failed")


def test_windows_derives_appdata_from_the_profile_when_unset():
    p = app_paths("windows", {"USERPROFILE": r"C:\Users\luke"})
    assert p.config_file == PureWindowsPath(r"C:\Users\luke\AppData\Roaming\wordink\dictate.toml")
    assert p.runtime_dir == PureWindowsPath(r"C:\Users\luke\AppData\Local\wordink\run")


def test_windows_without_any_profile_says_so():
    with pytest.raises(WordInkError, match="user profile"):
        app_paths("windows", {})


def test_unknown_os_is_rejected():
    with pytest.raises(WordInkError, match="unknown OS"):
        app_paths("plan9", {})


@pytest.mark.parametrize(
    ("platform", "expected"),
    [("linux", "linux"), ("win32", "windows"), ("cygwin", None), ("darwin", None)],
)
def test_current_os(platform, expected):
    if expected is None:
        with pytest.raises(WordInkError, match="supports Linux and Windows"):
            current_os(platform)
    else:
        assert current_os(platform) == expected


def test_protocols_are_structural():
    class Ins:
        def insert(self, text: str) -> str:
            return "fake"

    class Ind:
        def show(self, state: str, detail: str | None = None) -> None: ...

    assert isinstance(Ins(), interfaces.TextInserter)
    assert isinstance(Ind(), interfaces.Indicator)
    assert not isinstance(object(), interfaces.TextInserter)
