"""Where the dictation app keeps its files on each OS (Windows plan W1).

Pure functions of an OS name and an environment mapping, so the Windows layout is testable on Linux.
Nothing here touches the filesystem or an OS API.
"""

from __future__ import annotations

import sys
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import PurePath, PurePosixPath, PureWindowsPath

from ..client import WordInkError

OSName = str  # "linux" or "windows"


@dataclass(frozen=True)
class AppPaths:
    config_file: PurePath  # dictate.toml
    vocab_file: PurePath
    runtime_dir: PurePath  # state, last.txt, the socket (Linux) or the port + token (Windows)
    failed_dir: PurePath  # recordings kept after a failed transcription


def current_os(platform: str | None = None) -> OSName:
    p = sys.platform if platform is None else platform
    if p.startswith("win"):
        return "windows"
    if p.startswith("linux"):
        return "linux"
    raise WordInkError(f"wordink-dictd supports Linux and Windows, not {p!r}")


def _linux(env: Mapping[str, str], uid: int) -> AppPaths:
    home = PurePosixPath(env.get("HOME") or "~")
    config = PurePosixPath(env.get("XDG_CONFIG_HOME") or str(home / ".config")) / "wordink"
    cache = PurePosixPath(env.get("XDG_CACHE_HOME") or str(home / ".cache")) / "wordink"
    runtime = env.get("XDG_RUNTIME_DIR")
    return AppPaths(
        config_file=config / "dictate.toml",
        vocab_file=config / "vocabulary.txt",
        runtime_dir=PurePosixPath(runtime) / "wordink" if runtime else PurePosixPath(f"/tmp/wordink-{uid}"),
        failed_dir=cache / "failed",
    )


def _windows(env: Mapping[str, str]) -> AppPaths:
    profile = env.get("USERPROFILE")
    roaming = env.get("APPDATA") or (str(PureWindowsPath(profile) / "AppData" / "Roaming") if profile else None)
    local = env.get("LOCALAPPDATA") or (str(PureWindowsPath(profile) / "AppData" / "Local") if profile else None)
    if not roaming or not local:
        raise WordInkError("cannot find the user profile: APPDATA, LOCALAPPDATA and USERPROFILE are all unset")
    config = PureWindowsPath(roaming) / "wordink"
    data = PureWindowsPath(local) / "wordink"
    return AppPaths(
        config_file=config / "dictate.toml",
        vocab_file=config / "vocabulary.txt",
        runtime_dir=data / "run",
        failed_dir=data / "failed",
    )


def app_paths(os_name: OSName, env: Mapping[str, str], *, uid: int = 0) -> AppPaths:
    """The file layout for `os_name`. `uid` only names the `/tmp` fallback on Linux."""
    if os_name == "linux":
        return _linux(env, uid)
    if os_name == "windows":
        return _windows(env)
    raise WordInkError(f"unknown OS {os_name!r}; expected 'linux' or 'windows'")
