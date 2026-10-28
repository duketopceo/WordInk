"""The daemon's sink: last.txt first, then insertion, and [insert] config validation."""

from __future__ import annotations

import asyncio
import dataclasses

import pytest

from wordink.client import WordInkError
from wordink.desktop.config import load_config
from wordink.desktop.daemon import Daemon, PipelineResult, make_sink
from wordink.desktop.inject import Injector, inject_config_from
from wordink.desktop.indicator import StateFile
from wordink.desktop.recorder import Recorder

from dictd_helpers import FAKE_MIC, captured, short_tmp
from test_dictd_daemon import make_cfg, started, cmd
from test_dictd_inject import ALL, Rig


@pytest.fixture
def rig(tmp_path, monkeypatch):
    monkeypatch.setenv("FAKE", str(tmp_path))
    return Rig(tmp_path, ALL)


def test_sink_writes_last_txt_then_inserts(rig):
    root = short_tmp()
    cfg = make_cfg(root)
    rig.window("firefox")
    make_sink(cfg, Injector(path=str(rig.bin)))("hello world")
    assert cfg.last_path.read_text() == "hello world"
    assert rig.log == ["wtype -- hello world"]


def test_a_failed_insertion_keeps_the_text_in_last_txt(tmp_path, monkeypatch):
    monkeypatch.setenv("FAKE", str(tmp_path))
    root = short_tmp()
    cfg = make_cfg(root)
    empty = Rig(tmp_path, ["hyprctl"])
    empty.window("firefox")
    with pytest.raises(WordInkError):
        make_sink(cfg, Injector(path=str(empty.bin)))("do not lose me")
    assert cfg.last_path.read_text() == "do not lose me"


def test_daemon_end_to_end_types_the_dictation_and_reports_insertion_errors(tmp_path, monkeypatch):
    monkeypatch.setenv("FAKE", str(tmp_path))
    good = Rig(tmp_path, ALL)
    good.window("firefox")
    root = short_tmp()
    cfg = make_cfg(root)

    async def scenario(injector):
        daemon = await started(cfg, lambda wav: PipelineResult("typed by the daemon", False, "x"), make_sink(cfg, injector))
        try:
            await cmd(cfg, "start")
            await captured(daemon)
            await cmd(cfg, "stop")
            await daemon.wait_idle()
            return StateFile(cfg.state_path).read()
        finally:
            await daemon.close()

    state = asyncio.run(scenario(Injector(path=str(good.bin), sleep=lambda s: None)))
    assert state["state"] == "idle" and good.log == ["wtype -- typed by the daemon"]

    bare = tmp_path / "bare"
    bare.mkdir()
    state = asyncio.run(scenario(Injector(path=str(bare))))
    assert state["state"] == "error" and "install wtype" in state["detail"]
    assert cfg.last_path.read_text() == "typed by the daemon"


def _write(tmp_path, body):
    p = tmp_path / "dictate.toml"
    p.write_text(body)
    return p


def test_insert_table_flows_into_the_injector_config(tmp_path):
    cfg = load_config(
        _write(tmp_path, '[insert]\norder = ["ydotool"]\nterminals = ["MyTerm"]\ndeny = ["Vault"]\npaste_delay = 0.4\nnotify = false\n'),
        {"HOME": str(tmp_path)},
    )
    inj = inject_config_from(cfg.insert)
    assert inj.order == ("ydotool",) and inj.terminals == {"myterm"} and inj.deny == {"vault"}
    assert inj.paste_delay == 0.4 and inj.notify is False


def test_insert_defaults_when_absent(tmp_path):
    inj = inject_config_from(load_config(tmp_path / "none.toml", {"HOME": str(tmp_path)}).insert)
    assert inj.order == ("wtype", "ydotool") and "alacritty" in inj.terminals and "keepassxc" in inj.deny


@pytest.mark.parametrize(
    ("body", "message"),
    [
        ('[insert]\norder = ["xdotool"]', "order must be"),
        ("[insert]\norder = []", "order must be"),
        ("[insert]\nterminals = 'kitty'", "terminals must be a list"),
        ("[insert]\ndeny = [1]", "deny must be a list"),
        ("[insert]\npaste_delay = -1", "paste_delay"),
        ("[insert]\nnotify = 'yes'", "notify must be"),
        ("[insert]\nbogus = 1", "unknown key"),
    ],
)
def test_bad_insert_tables_fail_loudly(tmp_path, body, message):
    with pytest.raises(WordInkError, match=message):
        load_config(_write(tmp_path, body), {"HOME": str(tmp_path)})
