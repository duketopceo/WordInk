"""`~/.config/wordink/dictate.toml` and the runtime paths the daemon, the bind and the bar share."""

from __future__ import annotations

import os
import tomllib
from collections.abc import Mapping
from dataclasses import dataclass, field
from pathlib import Path

from ..client import WordInkError
from ..flow import FlowTone
from .cleanup import DEFAULT_LLM_MODEL, MIN_WORDS_FOR_LLM
from .transcribe import DEFAULT_WHISPER_MODEL
from .vocab import default_vocab_path

TONES: tuple[str, ...] = ("casual", "work", "email", "code", "verbatim")
DEFAULT_HOLD_BIND = "SUPER ALT, D"
DEFAULT_TOGGLE_BIND = "SUPER ALT, S"

_SCHEMA: dict[str, set[str]] = {
    "hotkey": {"hold", "toggle"},
    "transcribe": {"model", "language"},
    "cleanup": {"model", "tone", "timeout", "min_words"},
    "record": {"command", "min_seconds", "max_seconds", "tail_seconds"},
    "insert": {"order", "terminals", "deny", "paste_delay", "notify"},
}
INSERT_TOOLS = ("wtype", "ydotool")


@dataclass(frozen=True)
class DictateConfig:
    runtime_dir: Path
    vocab_path: Path
    failed_dir: Path
    hold_bind: str = DEFAULT_HOLD_BIND
    toggle_bind: str = DEFAULT_TOGGLE_BIND
    whisper_model: str | None = None
    language: str | None = None
    llm_model: str = DEFAULT_LLM_MODEL
    tone: FlowTone = "work"
    cleanup_timeout: float = 3.0
    min_words: int = MIN_WORDS_FOR_LLM
    record_command: tuple[str, ...] | None = None
    min_seconds: float = 0.3
    max_seconds: float = 60.0
    tail_seconds: float = 0.25
    insert: Mapping[str, object] = field(default_factory=dict)

    @property
    def socket_path(self) -> Path:
        return self.runtime_dir / "dictd.sock"

    @property
    def state_path(self) -> Path:
        return self.runtime_dir / "state"

    @property
    def last_path(self) -> Path:
        return self.runtime_dir / "last.txt"

    @property
    def whisper(self) -> str:
        return self.whisper_model or DEFAULT_WHISPER_MODEL


def default_runtime_dir(env: Mapping[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    base = e.get("XDG_RUNTIME_DIR")
    if base:
        return Path(base) / "wordink"
    return Path(f"/tmp/wordink-{os.getuid()}")


def default_config_path(env: Mapping[str, str] | None = None) -> Path:
    e = os.environ if env is None else env
    base = e.get("XDG_CONFIG_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".config")
    return Path(base) / "wordink" / "dictate.toml"


def _number(section: str, key: str, value: object, *, minimum: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value < minimum:
        raise WordInkError(f"dictate.toml [{section}] {key} must be a number >= {minimum:g}")
    return float(value)


def _string(section: str, key: str, value: object) -> str:
    if not isinstance(value, str) or not value.strip():
        raise WordInkError(f"dictate.toml [{section}] {key} must be a non-empty string")
    return value.strip()


def _insert_table(ins: dict) -> dict[str, object]:
    out: dict[str, object] = {}
    if "order" in ins:
        order = ins["order"]
        if not isinstance(order, list) or not order or any(t not in INSERT_TOOLS for t in order):
            raise WordInkError(f"dictate.toml [insert] order must be a non-empty list drawn from {', '.join(INSERT_TOOLS)}")
        out["order"] = list(order)
    for key in ("terminals", "deny"):
        if key in ins:
            val = ins[key]
            if not isinstance(val, list) or not all(isinstance(v, str) and v.strip() for v in val):
                raise WordInkError(f"dictate.toml [insert] {key} must be a list of window class names")
            out[key] = [v.strip() for v in val]
    if "paste_delay" in ins:
        out["paste_delay"] = _number("insert", "paste_delay", ins["paste_delay"], minimum=0.0)
    if "notify" in ins:
        if not isinstance(ins["notify"], bool):
            raise WordInkError("dictate.toml [insert] notify must be true or false")
        out["notify"] = ins["notify"]
    return out


def load_config(
    path: Path | None = None,
    env: Mapping[str, str] | None = None,
    *,
    os_name: str = "linux",
) -> DictateConfig:
    """Defaults, overridden by the TOML file when present. Bad keys and values fail loudly.

    `os_name="windows"` uses the Windows file layout, `Ctrl+Alt+D` style hotkeys and chord validation.
    """
    from .binds import validate_binds  # local import: binds imports nothing from config

    e = os.environ if env is None else env
    windows = os_name == "windows"
    if windows:
        from .platform import app_paths

        layout = app_paths("windows", e)
        cfg_path = path or Path(str(layout.config_file))
    else:
        cfg_path = path or default_config_path(e)
    data: dict = {}
    if cfg_path.exists():
        try:
            data = tomllib.loads(cfg_path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as exc:
            raise WordInkError(f"cannot read {cfg_path}: {exc}") from exc
    for section, body in data.items():
        if section not in _SCHEMA or not isinstance(body, dict):
            raise WordInkError(f"dictate.toml: unknown section [{section}]")
        for key in body:
            if key not in _SCHEMA[section]:
                raise WordInkError(f"dictate.toml [{section}]: unknown key {key!r}")
    hk, tr, cl, rc, ins = (data.get(s, {}) for s in ("hotkey", "transcribe", "cleanup", "record", "insert"))
    kw: dict[str, object] = {}
    if ins:
        kw["insert"] = _insert_table(ins)
    if "hold" in hk:
        kw["hold_bind"] = _string("hotkey", "hold", hk["hold"])
    if "toggle" in hk:
        kw["toggle_bind"] = _string("hotkey", "toggle", hk["toggle"])
    if "model" in tr:
        kw["whisper_model"] = _string("transcribe", "model", tr["model"])
    if "language" in tr:
        kw["language"] = _string("transcribe", "language", tr["language"])
    if "model" in cl:
        kw["llm_model"] = _string("cleanup", "model", cl["model"])
    if "tone" in cl:
        tone = _string("cleanup", "tone", cl["tone"])
        if tone not in TONES:
            raise WordInkError(f"dictate.toml [cleanup] tone must be one of {', '.join(TONES)}")
        kw["tone"] = tone
    if "timeout" in cl:
        kw["cleanup_timeout"] = _number("cleanup", "timeout", cl["timeout"], minimum=0.1)
    if "min_words" in cl:
        kw["min_words"] = int(_number("cleanup", "min_words", cl["min_words"], minimum=0))
    if "command" in rc:
        cmd = rc["command"]
        if not isinstance(cmd, list) or not cmd or not all(isinstance(a, str) and a for a in cmd):
            raise WordInkError("dictate.toml [record] command must be a non-empty list of strings")
        kw["record_command"] = tuple(cmd)
    for key in ("min_seconds", "max_seconds", "tail_seconds"):
        if key in rc:
            kw[key] = _number("record", key, rc[key], minimum=0.0 if key != "max_seconds" else 1.0)
    if windows:
        from .win.hotkey import DEFAULT_HOLD, DEFAULT_TOGGLE, parse_chords

        kw.setdefault("hold_bind", DEFAULT_HOLD)
        kw.setdefault("toggle_bind", DEFAULT_TOGGLE)
        cfg = DictateConfig(
            runtime_dir=Path(str(layout.runtime_dir)),
            vocab_path=Path(str(layout.vocab_file)),
            failed_dir=Path(str(layout.failed_dir)),
            **kw,  # type: ignore[arg-type]
        )
        parse_chords(cfg.hold_bind, cfg.toggle_bind)
        return cfg
    cfg = DictateConfig(
        runtime_dir=default_runtime_dir(e),
        vocab_path=default_vocab_path(dict(e)),
        failed_dir=Path(e.get("XDG_CACHE_HOME") or str(Path(e.get("HOME", "~")).expanduser() / ".cache")) / "wordink" / "failed",
        **kw,  # type: ignore[arg-type]
    )
    validate_binds(cfg.hold_bind, cfg.toggle_bind)
    return cfg
