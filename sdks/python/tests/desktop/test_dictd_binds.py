"""The Hyprland bind contract."""

from __future__ import annotations

import pytest

from wordink.client import WordInkError
from wordink.desktop.binds import hyprland_bind_lines, normalize_bind, validate_binds


def test_normalize_ignores_case_and_modifier_order():
    assert normalize_bind("super alt, d") == normalize_bind("ALT SUPER, D") == "ALT|SUPER|D"


@pytest.mark.parametrize("bad", ["", "SUPER", "SUPER ALT D", "SUPER, ", ", D", "SUPER, D, E", "HYPER, D", "D, D"])
def test_malformed_binds_are_rejected(bad):
    with pytest.raises(WordInkError):
        normalize_bind(bad)


def test_a_bare_key_with_no_modifier_is_rejected():
    with pytest.raises(WordInkError, match="at least one modifier"):
        normalize_bind(" , D")


def test_wisps_push_to_talk_key_is_reserved_in_any_spelling():
    for clash in ("SUPER, D", "super,d", "SUPER , D"):
        with pytest.raises(WordInkError, match="wisp"):
            validate_binds(clash, "SUPER ALT, S")
        with pytest.raises(WordInkError, match="wisp"):
            validate_binds("SUPER ALT, D", clash)


def test_super_alt_d_does_not_clash_with_wisp():
    validate_binds("SUPER ALT, D", "SUPER ALT, S")


def test_hold_and_toggle_must_differ():
    with pytest.raises(WordInkError, match="different"):
        validate_binds("SUPER ALT, S", "ALT SUPER, s")


def test_bind_lines_press_starts_release_stops_toggle_toggles():
    assert hyprland_bind_lines("SUPER ALT, D", "SUPER ALT, S") == [
        "bind = SUPER ALT, D, exec, wordink-dictd trigger start",
        "bindr = SUPER ALT, D, exec, wordink-dictd trigger stop",
        "bind = SUPER ALT, S, exec, wordink-dictd trigger toggle",
    ]


def test_bind_lines_honour_the_executable_path():
    lines = hyprland_bind_lines("SUPER SHIFT, V", "SUPER SHIFT, B", exe="/home/u/.local/bin/wordink-dictd")
    assert all("/home/u/.local/bin/wordink-dictd trigger" in line for line in lines)


def test_lua_lines_use_omarchy_o_bind_with_an_absolute_exe_and_a_release_bind():
    from wordink.desktop.binds import hyprland_lua_lines

    lines = hyprland_lua_lines("SUPER ALT, D", "SUPER ALT, V", "/home/u/.local/bin/wordink-dictd")
    assert lines[0] == 'o.bind("SUPER + ALT + D", "WordInk dictation (hold)", { launch = "/home/u/.local/bin/wordink-dictd trigger start" })'
    assert lines[1].endswith('{ release = true })') and "trigger stop" in lines[1]
    assert lines[2].startswith('o.bind("SUPER + ALT + V"') and "trigger toggle" in lines[2]


def test_lua_lines_refuse_wisps_key():
    from wordink.desktop.binds import hyprland_lua_lines

    with pytest.raises(WordInkError, match="wisp"):
        hyprland_lua_lines("SUPER, D", "SUPER ALT, V")
