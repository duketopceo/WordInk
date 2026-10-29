"""macOS insertion policy against a fake native layer (plan M4)."""

from __future__ import annotations

import sys

import pytest

from wordink.client import WordInkError
from wordink.desktop.mac.appkit import has_other_formats
from wordink.desktop.mac.insert import (
    KVK_RETURN, MacInserter, MacInsertConfig, NativeInsertApi, PasteboardBusy, PasteboardSnapshot, chunks,
    mac_insert_config_from, plan_events,
)
from wordink.desktop.mac.quartz import split_batch, units_to_text


class FakeApi:
    def __init__(self, bundle="com.apple.TextEdit", secure=False, ax=True, board="old", other=False):
        self.bundle, self.secure, self.ax, self.board, self.other = bundle, secure, ax, board, other
        self.calls: list[tuple] = []
        self.busy_times = 0
        self.fail_typing = False
        self.fail_paste = False

    def frontmost_bundle_id(self):
        return self.bundle

    def secure_input_enabled(self):
        return self.secure

    def accessibility_trusted(self):
        return self.ax

    def release_modifiers(self, wait):
        self.calls.append(("release", wait))

    def send_events(self, events):
        if self.fail_typing:
            raise OSError("blocked")
        self.calls.append(("events", list(events)))

    def send_paste(self):
        if self.fail_paste:
            raise OSError("blocked")
        self.calls.append(("paste",))

    def _busy(self):
        if self.busy_times:
            self.busy_times -= 1
            raise PasteboardBusy()

    def pasteboard_snapshot(self):
        self._busy()
        return PasteboardSnapshot(self.board, self.other)

    def pasteboard_set(self, text):
        self._busy()
        self.board, self.other = text, False
        self.calls.append(("set", text))

    def pasteboard_clear(self):
        self.board = None
        self.calls.append(("clear",))

    def typed(self):
        units = [u for c in self.calls if c[0] == "events" for k, u in c[1] if k == "unit"]
        return units_to_text(units)

    def kinds(self):
        return [c[0] for c in self.calls]


class Later:
    def __init__(self):
        self.jobs = []

    def __call__(self, delay, fn):
        self.jobs.append(fn)

    def run_all(self):
        jobs, self.jobs = self.jobs, []
        for j in jobs:
            j()


def make(api=None, cfg=None):
    api = api or FakeApi()
    later, notes = Later(), []
    ins = MacInserter(api, cfg, notifier=notes.append, sleep=lambda s: None, schedule=later)
    return ins, api, later, notes


def test_ascii_and_emoji_type_as_utf16_units_without_splitting_pairs():
    ins, api, _, _ = make()
    r = ins.insert("hi 😀!")
    assert (r.method, api.typed()) == ("typed", "hi 😀!")
    assert all(len(c[1]) <= 20 for c in api.calls if c[0] == "events")
    assert ("release", 0.3) in api.calls and api.calls[0][0] == "release"  # modifiers go up first


def test_chunks_never_split_a_surrogate_pair():
    ev = plan_events("a" * 19 + "😀" + "b")  # the emoji's high surrogate lands on unit 20
    for batch in chunks(ev, 20):
        assert not (batch[-1][0] == "unit" and 0xD800 <= batch[-1][1] <= 0xDBFF)
    assert sum(len(b) for b in chunks(ev, 20)) == len(ev)


def test_every_newline_form_is_one_return():
    assert plan_events("a\r\nb\rc\nd").count(("vk", KVK_RETURN)) == 3


def test_a_long_text_is_pasted_then_the_pasteboard_is_restored():
    ins, api, later, _ = make()
    text = "x" * 201
    r = ins.insert(text)
    assert r.method == "pasted" and api.kinds() == ["set", "release", "paste"] and api.board == text
    later.run_all()
    assert api.board == "old" and api.typed() == ""


def test_a_terminal_bundle_id_pastes_even_short_text():
    ins, api, _, _ = make(FakeApi(bundle="com.googlecode.iterm2"))
    assert ins.insert("ls -la").method == "pasted"


def test_secure_input_copies_and_notifies_and_never_types():
    ins, api, later, notes = make(FakeApi(secure=True))
    r = ins.insert("secret words")
    assert r.method == "clipboard" and api.board == "secret words" and "events" not in api.kinds()
    assert len(notes) == 1 and "secure field" in notes[0]
    later.run_all()
    assert api.board == "secret words"  # no restore may clobber it


def test_no_accessibility_explains_once_then_stays_quiet():
    ins, api, _, notes = make(FakeApi(ax=False))
    assert ins.insert("one").method == "clipboard"
    assert ins.insert("two").method == "clipboard"
    assert api.board == "two" and len(notes) == 1 and "Accessibility" in notes[0]
    assert "events" not in api.kinds() and "paste" not in api.kinds()


def test_a_password_manager_is_skipped_without_typing_or_touching_the_pasteboard():
    for bundle in ("com.1password.1password", "com.bitwarden.desktop", "org.keepassxc.keepassxc"):
        ins, api, _, notes = make(FakeApi(bundle=bundle))
        r = ins.insert("hunter2")
        assert r.method == "skipped" and api.calls == [] and api.board == "old" and notes


def test_a_custom_deny_and_terminal_list_comes_from_the_config_table():
    cfg = mac_insert_config_from({"deny": ["com.example.Vault"], "terminals": ["com.example.Term"], "notify": False})
    ins, api, _, notes = make(FakeApi(bundle="com.example.vault"), cfg)
    assert ins.insert("x").method == "skipped" and notes == []
    ins, api, _, _ = make(FakeApi(bundle="com.example.term"), cfg)
    assert ins.insert("x").method == "pasted"
    ins, api, _, _ = make(FakeApi(bundle="com.apple.Terminal"), cfg)
    assert ins.insert("x").method == "typed"  # the default list was replaced


def test_non_text_on_the_pasteboard_prevents_paste_and_types_instead():
    ins, api, later, _ = make(FakeApi(other=True))
    r = ins.insert("y" * 300)
    assert r.method == "typed" and "set" not in api.kinds()
    later.run_all()
    assert api.other is True


def test_a_user_copy_before_the_restore_is_left_alone():
    ins, api, later, _ = make()
    ins.insert("z" * 300)
    api.board = "the user copied this"
    later.run_all()
    assert api.board == "the user copied this"


def test_a_second_dictation_cancels_the_first_restore_and_restores_the_original_once():
    ins, api, later, _ = make()
    ins.insert("a" * 300)
    ins.insert("b" * 300)
    first, second = later.jobs
    first()
    assert api.board == "b" * 300  # the stale restore did nothing
    second()
    assert api.board == "old"


def test_an_empty_original_pasteboard_is_cleared_on_restore():
    ins, api, later, _ = make(FakeApi(board=None))
    ins.insert("q" * 300)
    later.run_all()
    assert api.board is None


def test_a_refused_paste_restores_and_falls_back_to_typing():
    api = FakeApi()
    api.fail_paste = True
    ins, _, _, _ = make(api)
    r = ins.insert("w" * 300)
    assert r.method == "typed" and api.board == "old"


def test_a_busy_pasteboard_is_retried_and_a_stuck_one_falls_back_to_typing():
    api = FakeApi()
    api.busy_times = 2
    ins, _, _, _ = make(api)
    assert ins.insert("m" * 300).method == "pasted"
    api = FakeApi()
    api.busy_times = 99
    ins, _, _, _ = make(api)
    assert ins.insert("m" * 300).method == "typed"


def test_a_stuck_pasteboard_on_hand_over_raises_a_clear_error():
    api = FakeApi(secure=True)
    api.busy_times = 99
    ins, _, _, _ = make(api)
    with pytest.raises(WordInkError, match="wordink-dictd last"):
        ins.insert("hello")


def test_typing_failure_hands_the_text_over_on_the_pasteboard():
    api = FakeApi()
    api.fail_typing = True
    ins, _, _, notes = make(api)
    r = ins.insert("hello")
    assert r.method == "clipboard" and api.board == "hello" and "typing failed" in notes[0]


def test_empty_text_is_skipped_and_notify_false_is_silent():
    ins, api, _, notes = make(FakeApi(secure=True), MacInsertConfig(notify=False))
    assert ins.insert("").method == "skipped"
    ins.insert("abc")
    assert notes == []


def test_has_other_formats_ignores_text_spellings_and_markers():
    assert not has_other_formats(["public.utf8-plain-text", "NSStringPboardType", "org.nspasteboard.ConcealedType"])
    assert has_other_formats(["public.utf8-plain-text", "public.png"])
    assert has_other_formats(["public.rtf"])


def test_split_batch_groups_units_and_keeps_return_in_order():
    ev = plan_events("hi\n😀")
    assert split_batch(ev) == [("text", "hi"), ("vk", KVK_RETURN), ("text", "😀")]


@pytest.mark.skipif(sys.platform == "darwin", reason="the real frameworks exist here")
def test_the_native_api_loads_off_a_mac_and_only_fails_when_called():
    api = NativeInsertApi()
    with pytest.raises((OSError, ImportError)):
        api.secure_input_enabled()
