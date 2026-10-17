"""Cross-language Flow contract tests for sdks/python against tests/fixtures/flow-contract.json."""

from __future__ import annotations

import json
from pathlib import Path

from wordink.cli import main
from wordink.flow import (
    apply_flow_cleanup,
    apply_tone_preset,
    expand_snippets,
    format_spoken_lists,
    learn_vocabulary_from_edit,
    resolve_course_corrections,
    strip_fillers,
)

CONTRACT_PATH = (
    Path(__file__).resolve().parents[3] / "tests" / "fixtures" / "flow-contract.json"
)
CONTRACT = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))


def test_strip_fillers_contract():
    for case in CONTRACT["stripFillers"]:
        assert strip_fillers(case["input"]) == case["expected"], case["id"]


def test_resolve_course_corrections_contract():
    for case in CONTRACT["resolveCourseCorrections"]:
        assert (
            resolve_course_corrections(case["input"]) == case["expected"]
        ), case["id"]


def test_format_spoken_lists_contract():
    for case in CONTRACT["formatSpokenLists"]:
        assert format_spoken_lists(case["input"]) == case["expected"], case["id"]


def test_expand_snippets_contract():
    for case in CONTRACT["expandSnippets"]:
        assert (
            expand_snippets(case["input"], case["snippets"]) == case["expected"]
        ), case["id"]


def test_apply_tone_preset_contract():
    for case in CONTRACT["applyTonePreset"]:
        assert (
            apply_tone_preset(case["input"], case["tone"]) == case["expected"]
        ), case["id"]


def test_apply_flow_cleanup_contract():
    for case in CONTRACT["applyFlowCleanup"]:
        assert (
            apply_flow_cleanup(case["input"], case["options"]) == case["expected"]
        ), case["id"]


def test_learn_vocabulary_from_edit_contract():
    for case in CONTRACT["learnVocabularyFromEdit"]:
        diff = learn_vocabulary_from_edit(case["dictated"], case["edited"])
        assert diff.to_contract_dict() == case["expected"], case["id"]


def test_cli_flow_cleanup_and_tone(monkeypatch, tmp_path, capsys):
    monkeypatch.setenv("WORDINK_DEVICE_TOKEN", "wdk_test")
    audio = tmp_path / "sample.wav"
    audio.write_bytes(b"RIFF....")
    monkeypatch.setattr(
        "wordink.cli.transcribe",
        lambda *a, **kw: "um let us meet at 2 pm no wait 3 pm",
    )

    assert main(["transcribe", str(audio), "--flow-cleanup", "--tone", "work"]) == 0
    assert capsys.readouterr().out.strip() == "Let us meet at 3 pm."

    assert main(["transcribe", str(audio), "--tone", "casual"]) == 0
    assert capsys.readouterr().out.strip() == "let us meet at 3 pm"
