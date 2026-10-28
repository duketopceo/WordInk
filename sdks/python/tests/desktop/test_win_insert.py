"""Windows insertion policy against a fake Win32 layer (plan W5)."""

from __future__ import annotations

import pytest

from wordink.client import WordInkError
from wordink.desktop.win.insert import (
    VK_CONTROL, VK_RETURN, VK_SHIFT, VK_V, ClipboardBusy, ClipboardSnapshot, WinInsertConfig, WindowsInserter,
    _chunks, plan_events, win_insert_config_from,
)


class FakeApi:
    def __init__(self, proc="notepad.exe", fg_elevated=False, self_elevated=False, clip="old", other=False) -> None:
        self.proc, self.fg_elevated, self._self_elevated = proc, fg_elevated, self_elevated
        self.clip, self.other = clip, other
        self.calls: list[tuple] = []
        self.busy_times = 0
        self.fail_typing = False

    def foreground_process(self):
        return self.proc

    def foreground_elevated(self):
        return self.fg_elevated

    def self_elevated(self):
        return self._self_elevated

    def release_modifiers(self, wait):
        self.calls.append(("release", wait))

    def send_events(self, events):
        if self.fail_typing:
            raise OSError("SendInput blocked")
        self.calls.append(("events", list(events)))

    def send_chord(self, vks):
        self.calls.append(("chord", tuple(vks)))

    def clipboard_snapshot(self):
        if self.busy_times:
            self.busy_times -= 1
            raise ClipboardBusy()
        return ClipboardSnapshot(self.clip, self.other)

    def clipboard_set(self, text):
        if self.busy_times:
            self.busy_times -= 1
            raise ClipboardBusy()
        self.clip, self.other = text, False
        self.calls.append(("set", text))

    def clipboard_clear(self):
        self.clip = None
        self.calls.append(("clear",))

    def typed_units(self):
        return [u for c in self.calls if c[0] == "events" for k, u in c[1] if k == "unit"]


class Sched:
    def __init__(self) -> None:
        self.jobs: list = []

    def __call__(self, delay, fn):
        self.jobs.append(fn)

    def run_all(self):
        jobs, self.jobs = self.jobs, []
        for j in jobs:
            j()


def make(api=None, cfg=None):
    api = api or FakeApi()
    sched, said = Sched(), []
    ins = WindowsInserter(api, cfg, notifier=said.append, sleep=lambda s: None, schedule=sched)
    return api, ins, sched, said


def test_ascii_is_typed_after_modifiers_are_released():
    api, ins, _, _ = make()
    res = ins.insert("hi")
    assert (res.method, res.detail) == ("typed", "SendInput")
    assert api.calls[0] == ("release", 0.3)
    assert api.typed_units() == [ord("h"), ord("i")]


def test_emoji_goes_out_as_one_surrogate_pair_even_across_a_chunk_boundary():
    assert plan_events("😀") == [("unit", 0xD83D), ("unit", 0xDE00)]
    events = plan_events("a" * 49 + "😀" + "b")
    batches = _chunks(events, 50)
    assert [len(b) for b in batches] == [51, 1]  # the pair stays together
    assert all(not (b[-1][0] == "unit" and 0xD800 <= b[-1][1] <= 0xDBFF) for b in batches)


@pytest.mark.parametrize("text", ["a\nb", "a\r\nb", "a\rb"])
def test_every_newline_form_is_one_enter(text):
    assert plan_events(text) == [("unit", ord("a")), ("vk", VK_RETURN), ("unit", ord("b"))]


def test_long_text_is_sent_in_chunks_of_fifty():
    api, ins, _, _ = make(cfg=WinInsertConfig(long_text=10_000))
    ins.insert("x" * 120)
    sizes = [len(c[1]) for c in api.calls if c[0] == "events"]
    assert sizes == [50, 50, 20]


def test_a_long_text_is_pasted_with_ctrl_v_and_the_clipboard_comes_back():
    api, ins, sched, _ = make(cfg=WinInsertConfig(long_text=20))
    res = ins.insert("y" * 30)
    assert res.method == "pasted"
    assert ("chord", (VK_CONTROL, VK_V)) in api.calls
    assert api.clip == "y" * 30
    sched.run_all()
    assert api.clip == "old"


def test_a_terminal_pastes_with_ctrl_shift_v():
    api, ins, sched, _ = make(FakeApi(proc="WindowsTerminal.exe"))
    res = ins.insert("ls -la")
    assert res.method == "pasted"
    assert ("chord", (VK_CONTROL, VK_SHIFT, VK_V)) in api.calls
    sched.run_all()
    assert api.clip == "old"


def test_an_empty_clipboard_is_cleared_again_after_a_paste():
    api, ins, sched, _ = make(FakeApi(proc="cmd.exe", clip=None))
    ins.insert("dir")
    sched.run_all()
    assert api.clip is None and ("clear",) in api.calls


def test_the_restore_leaves_a_clipboard_the_user_changed_meanwhile():
    api, ins, sched, _ = make(FakeApi(proc="cmd.exe"))
    ins.insert("dir")
    api.clip = "user copied this"
    sched.run_all()
    assert api.clip == "user copied this"


def test_a_second_dictation_cancels_the_first_restore_and_restores_the_original_once():
    api, ins, sched, _ = make(FakeApi(proc="cmd.exe"))
    ins.insert("first")
    ins.insert("second")
    assert len(sched.jobs) == 2
    sched.run_all()  # first's restore is stale, second's restores the user's original
    assert api.clip == "old"
    assert [c for c in api.calls if c[0] == "set"][-1] == ("set", "old")


def test_non_text_clipboard_blocks_paste_and_falls_back_to_typing():
    api, ins, sched, _ = make(FakeApi(proc="cmd.exe", other=True))
    res = ins.insert("dir")
    assert res.method == "typed" and not sched.jobs and api.clip == "old"


def test_a_password_manager_is_never_typed_into():
    api, ins, _, said = make(FakeApi(proc="1Password.exe"))
    res = ins.insert("hunter2")
    assert res.method == "skipped" and not api.calls and said and "password manager" in said[0]


def test_an_elevated_window_gets_the_clipboard_and_a_toast_not_a_silent_failure():
    api, ins, sched, said = make(FakeApi(fg_elevated=True))
    res = ins.insert("hello")
    assert res.method == "clipboard" and api.clip == "hello" and "administrator" in said[0]
    assert not [c for c in api.calls if c[0] in ("events", "chord")]
    sched.run_all()
    assert api.clip == "hello"  # nothing restores over the handed-over text


def test_an_elevated_daemon_can_type_into_an_elevated_window():
    api, ins, _, _ = make(FakeApi(fg_elevated=True, self_elevated=True))
    assert ins.insert("hello").method == "typed"


def test_the_history_exclusion_is_the_apis_job_on_every_clipboard_write():
    # the inserter only ever writes through clipboard_set, which the real layer implements with
    # the ExcludeClipboardContentFromMonitorProcessing / CanIncludeInClipboardHistory=0 /
    # CanUploadToCloudClipboard=0 formats; there is no other write path
    api, ins, sched, _ = make(FakeApi(proc="cmd.exe"))
    ins.insert("dir")
    sched.run_all()
    assert {c[0] for c in api.calls} <= {"release", "chord", "set", "clear", "events"}


def test_a_busy_clipboard_is_retried_then_succeeds():
    api = FakeApi(proc="cmd.exe")
    api.busy_times = 3
    _, ins, _, _ = make(api)
    assert ins.insert("dir").method == "pasted"


def test_a_clipboard_that_stays_busy_falls_back_to_typing():
    api = FakeApi(proc="cmd.exe")
    api.busy_times = 99
    _, ins, sched, _ = make(api)
    assert ins.insert("dir").method == "typed" and not sched.jobs


def test_typing_failure_hands_the_text_over_on_the_clipboard():
    api = FakeApi()
    api.fail_typing = True
    _, ins, _, said = make(api)
    res = ins.insert("hello")
    assert res.method == "clipboard" and api.clip == "hello" and "typing failed" in said[0]


def test_nothing_works_raises_with_a_pointer_to_last():
    api = FakeApi(fg_elevated=True)
    api.busy_times = 99
    _, ins, _, _ = make(api)
    with pytest.raises(WordInkError, match="wordink-dictd last"):
        ins.insert("hello")


def test_empty_text_is_skipped_and_config_comes_from_the_toml_table():
    api, ins, _, _ = make()
    assert ins.insert("").method == "skipped" and not api.calls
    cfg = win_insert_config_from({"deny": ["MyVault.exe"], "terminals": ["Hyper.exe"], "paste_delay": 0.5, "notify": False})
    assert cfg.deny == {"myvault.exe"} and cfg.terminals == {"hyper.exe"} and cfg.paste_delay == 0.5 and not cfg.notify
    _, ins2, _, said = make(FakeApi(proc="myvault.exe"), cfg)
    assert ins2.insert("x").method == "skipped" and not said  # notify off


def test_a_failed_paste_chord_restores_the_clipboard_and_types_instead():
    api = FakeApi(proc="cmd.exe")

    def boom(vks):
        raise OSError("blocked")

    api.send_chord = boom
    _, ins, sched, _ = make(api)
    res = ins.insert("dir")
    assert res.method == "typed" and api.clip == "old" and not sched.jobs
