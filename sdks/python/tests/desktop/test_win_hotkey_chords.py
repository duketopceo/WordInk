"""Windows hotkey chord parsing (plan W2)."""

from __future__ import annotations

import pytest

from wordink.client import WordInkError
from wordink.desktop.win.hotkey import DEFAULT_HOLD, DEFAULT_TOGGLE, VK_CONTROL, VK_MENU, parse_chord, parse_chords


def test_the_defaults_parse():
    h, t = parse_chords(DEFAULT_HOLD, DEFAULT_TOGGLE)
    assert (h.text, h.key, h.modifiers) == ("Alt+Ctrl+D", ord("D"), ("Alt", "Ctrl"))
    assert t.key == ord("S")
    assert set(h.modifier_vks) == {VK_CONTROL, VK_MENU}


@pytest.mark.parametrize(
    ("chord", "key"),
    [("Ctrl+Alt+7", ord("7")), ("ctrl+shift+space", 0x20), ("Ctrl+F1", 0x70), ("Alt+f24", 0x87), ("Ctrl+Pause", 0x13),
     ("Ctrl+Alt+PageUp", 0x21), ("Control+Alt+Z", ord("Z"))],
)
def test_keys(chord, key):
    assert parse_chord(chord).key == key


def test_modifier_order_and_case_do_not_matter():
    assert parse_chord("alt+CTRL+d") == parse_chord("Ctrl+Alt+D")
    assert parse_chord(" Ctrl + Alt + D ").text == "Alt+Ctrl+D"


@pytest.mark.parametrize("chord", ["Win+D", "Ctrl+Win+D", "Super+Alt+D", "Meta+D"])
def test_win_combinations_are_rejected(chord):
    with pytest.raises(WordInkError, match="Win key"):
        parse_chord(chord)


@pytest.mark.parametrize("chord", ["D", "", "Ctrl+", "+D", "Ctrl+Alt", "Ctrl+Alt+Shift", "Ctrl++D"])
def test_bare_and_malformed_chords_are_rejected(chord):
    with pytest.raises(WordInkError):
        parse_chord(chord)


@pytest.mark.parametrize("chord", ["Hyper+D", "Ctrl+Alt+Banana", "Ctrl+F25", "Ctrl+F0", "Ctrl+Ä"])
def test_unknown_names_are_rejected(chord):
    with pytest.raises(WordInkError):
        parse_chord(chord)


@pytest.mark.parametrize("chord", ["Alt+F4", "Alt+Tab", "Ctrl+Alt+Delete", "Ctrl+Shift+Esc", "Ctrl+C", "Ctrl+V"])
def test_reserved_chords_are_rejected(chord):
    with pytest.raises(WordInkError, match="reserved"):
        parse_chord(chord)


def test_hold_equal_to_toggle_is_rejected():
    with pytest.raises(WordInkError, match="different"):
        parse_chords("Ctrl+Alt+D", "alt+ctrl+d")
