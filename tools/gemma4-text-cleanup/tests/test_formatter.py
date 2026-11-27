from wordink_gemma4.formatter import (
    build_rewrite_prompt,
    format_record,
    vocabulary_lines,
    VocabularyEntry,
)


def test_vocabulary_lines_empty():
    assert vocabulary_lines([]) == ""


def test_vocabulary_lines_aliases():
    text = vocabulary_lines(
        [VocabularyEntry("OpenCode", ("open code",)), VocabularyEntry("Hex", ())]
    )
    assert "OpenCode (may be transcribed as open code)" in text
    assert "Hex" in text


def test_build_rewrite_prompt_wraps_transcript():
    prompt = build_rewrite_prompt(
        profile_prompt="Fix it.",
        raw_transcript="um hello",
        application="Slack",
        vocabulary=[VocabularyEntry("WordInk", ())],
    )
    assert "Profile instructions:\nFix it." in prompt
    assert "<transcript>\num hello\n</transcript>" in prompt
    assert "Foreground application: Slack" in prompt


def test_format_record_chat():
    row = format_record("user", "assistant", style="chat")
    assert row["messages"][0]["content"] == "user"
    assert row["messages"][1]["content"] == "assistant"