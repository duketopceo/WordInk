"""The Hyprland bind contract: what the compositor runs, and which keys are off limits."""

from __future__ import annotations

from ..client import WordInkError

#: Keys another resident tool owns. wisp holds `SUPER, D` for push-to-talk on the same machine.
RESERVED_BINDS = frozenset({"SUPER|D"})
_MODIFIERS = {"SUPER", "ALT", "CTRL", "SHIFT", "MOD2", "MOD3", "MOD5"}


def normalize_bind(bind: str) -> str:
    """`"SUPER ALT, D"` -> `"ALT|SUPER|D"`: modifier order and case do not matter to Hyprland."""
    mods_part, sep, key = bind.partition(",")
    key = key.strip()
    if not sep or not key or "," in key or " " in key:
        raise WordInkError(f"bind {bind!r} must look like 'SUPER ALT, D' (modifiers, then one key)")
    mods = sorted({m.upper() for m in mods_part.replace("_", " ").split()})
    unknown = [m for m in mods if m not in _MODIFIERS]
    if unknown:
        raise WordInkError(f"bind {bind!r} has unknown modifier {unknown[0]!r}")
    if not mods:
        raise WordInkError(f"bind {bind!r} needs at least one modifier so normal typing is not captured")
    return "|".join([*mods, key.upper()])


def validate_binds(hold: str, toggle: str) -> None:
    """Both binds parse, differ from each other, and avoid the keys wisp already owns."""
    h, t = normalize_bind(hold), normalize_bind(toggle)
    if h == t:
        raise WordInkError("the hold and toggle binds must be different keys")
    for label, norm, raw in (("hold", h, hold), ("toggle", t, toggle)):
        if norm in RESERVED_BINDS:
            raise WordInkError(f"the {label} bind {raw!r} is wisp's push-to-talk key; pick another")


def hyprland_bind_lines(hold: str, toggle: str, exe: str = "wordink-dictd") -> list[str]:
    """The `hyprland.conf` lines: press starts, release stops (`bindr`), a second key toggles."""
    validate_binds(hold, toggle)
    return [
        f"bind = {hold}, exec, {exe} trigger start",
        f"bindr = {hold}, exec, {exe} trigger stop",
        f"bind = {toggle}, exec, {exe} trigger toggle",
    ]
