"""The hotkey state machine: hold, toggle, swallowing, injected input (plan W4a)."""

from __future__ import annotations

import time

from wordink.desktop.win.hotkey import HOLD_PRESS, HOLD_RELEASE, TOGGLE, ChordTracker, parse_chords

LCTRL, RCTRL, LALT, LSHIFT = 0xA2, 0xA3, 0xA4, 0xA0
D, S, A = ord("D"), ord("S"), ord("A")


def tracker() -> ChordTracker:
    return ChordTracker(*parse_chords("Ctrl+Alt+D", "Ctrl+Alt+S"))


def run(t, steps):
    """steps: (vk, 'down'|'up'[, 'inj']) -> list of (swallow, events)"""
    results = [t.feed(s[0], s[1] == "down", len(s) > 2) for s in steps]
    return [(r.swallow, r.events) for r in results]


def test_a_clean_hold_presses_then_releases_and_swallows_only_the_main_key():
    out = run(tracker(), [(LCTRL, "down"), (LALT, "down"), (D, "down"), (D, "up"), (LALT, "up"), (LCTRL, "up")])
    assert out == [
        (False, ()), (False, ()),
        (True, (HOLD_PRESS,)),
        (True, (HOLD_RELEASE,)),
        (False, ()), (False, ()),
    ]


def test_releasing_a_modifier_first_ends_the_hold_and_the_late_key_up_is_still_swallowed():
    t = tracker()
    out = run(t, [(LCTRL, "down"), (LALT, "down"), (D, "down"), (LALT, "up"), (D, "up"), (LCTRL, "up")])
    assert out[3] == (False, (HOLD_RELEASE,))
    assert out[4] == (True, ())  # the D key-up we owe the app
    assert not t.holding


def test_key_repeat_while_held_is_swallowed_without_new_events():
    t = tracker()
    run(t, [(LCTRL, "down"), (LALT, "down"), (D, "down")])
    assert run(t, [(D, "down"), (D, "down"), (D, "down")]) == [(True, ())] * 3
    assert t.holding


def test_other_keys_typed_during_the_hold_pass_through():
    t = tracker()
    run(t, [(LCTRL, "down"), (LALT, "down"), (D, "down")])
    assert run(t, [(A, "down"), (A, "up")]) == [(False, ()), (False, ())]
    assert t.holding


def test_the_key_alone_or_with_the_wrong_modifiers_is_not_a_chord():
    t = tracker()
    assert run(t, [(D, "down"), (D, "up")]) == [(False, ()), (False, ())]
    assert run(t, [(LCTRL, "down"), (D, "down"), (D, "up"), (LCTRL, "up")])[1:3] == [(False, ()), (False, ())]
    assert run(t, [(LCTRL, "down"), (LALT, "down"), (LSHIFT, "down"), (D, "down")])[3] == (False, ())  # extra Shift


def test_injected_events_are_ignored_entirely():
    t = tracker()
    out = run(t, [(LCTRL, "down", "inj"), (LALT, "down", "inj"), (D, "down", "inj"), (D, "up", "inj")])
    assert out == [(False, ())] * 4 and not t.holding
    # and they do not count as held modifiers for a real D afterwards
    assert run(t, [(D, "down")]) == [(False, ())]


def test_the_toggle_chord_fires_once_per_press_and_swallows_its_key_up():
    t = tracker()
    out = run(t, [(LCTRL, "down"), (LALT, "down"), (S, "down"), (S, "down"), (S, "up"), (S, "down"), (S, "up")])
    assert out[2] == (True, (TOGGLE,))
    assert out[3] == (True, ())  # repeat
    assert out[4] == (True, ())
    assert out[5] == (True, (TOGGLE,)) and out[6] == (True, ())


def test_both_ctrl_keys_count_as_one_modifier_until_both_are_up():
    t = tracker()
    run(t, [(LCTRL, "down"), (RCTRL, "down"), (LALT, "down"), (D, "down")])
    assert t.holding
    assert run(t, [(LCTRL, "up")]) == [(False, ())]  # right Ctrl still down
    assert run(t, [(RCTRL, "up")]) == [(False, (HOLD_RELEASE,))]


def test_reset_forgets_stuck_state():
    t = tracker()
    run(t, [(LCTRL, "down"), (LALT, "down"), (D, "down")])
    t.reset()
    assert not t.holding
    assert run(t, [(D, "up")]) == [(False, ())]  # nothing owed after a reset


def test_the_callback_path_is_fast():
    t = tracker()
    start = time.perf_counter()
    for _ in range(10_000):
        t.feed(A, True)
        t.feed(A, False)
    assert time.perf_counter() - start < 0.5  # 20k events well under a hook's budget
