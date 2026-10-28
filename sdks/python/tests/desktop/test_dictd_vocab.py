"""Vocabulary file, gateway merge, prompt packing and spelling enforcement."""

from __future__ import annotations

import httpx

from wordink.desktop.vocab import (
    add_terms,
    apply_vocabulary,
    default_vocab_path,
    load_vocabulary,
    merge_terms,
    pack_prompt,
    read_terms,
    remove_term,
)


def test_default_path_follows_xdg(tmp_path):
    assert default_vocab_path({"XDG_CONFIG_HOME": str(tmp_path)}) == tmp_path / "wordink" / "vocabulary.txt"
    assert default_vocab_path({"HOME": "/home/u"}).as_posix() == "/home/u/.config/wordink/vocabulary.txt"


def test_read_missing_file_is_empty(tmp_path):
    assert read_terms(tmp_path / "nope.txt") == []


def test_add_dedupes_case_insensitively_and_keeps_first_spelling(tmp_path):
    path = tmp_path / "v" / "vocabulary.txt"
    assert add_terms(path, ["Hyprland", "omarchy", " WordInk "]) == ["Hyprland", "omarchy", "WordInk"]
    assert add_terms(path, ["HYPRLAND", "Omarchy", "Tailscale"]) == ["Tailscale"]
    assert read_terms(path) == ["Hyprland", "omarchy", "WordInk", "Tailscale"]


def test_read_ignores_comments_and_blanks(tmp_path):
    path = tmp_path / "v.txt"
    path.write_text("# my terms\nWordInk  # product\n\n  \nVoxtype\n", encoding="utf-8")
    assert read_terms(path) == ["WordInk", "Voxtype"]


def test_remove_term(tmp_path):
    path = tmp_path / "v.txt"
    add_terms(path, ["WordInk", "Voxtype"])
    assert remove_term(path, "voxtype") is True
    assert remove_term(path, "voxtype") is False
    assert read_terms(path) == ["WordInk"]


def test_merge_terms_first_spelling_wins():
    assert merge_terms(["WordInk"], ["wordink", "Groq"], ["GROQ"]) == ["WordInk", "Groq"]


def test_pack_prompt_keeps_whole_terms_within_budget():
    terms = ["alpha", "beta", "gamma"]
    assert pack_prompt(terms) == "alpha, beta, gamma"
    assert pack_prompt(terms, max_chars=12) == "alpha, beta"
    assert pack_prompt(["x" * 50, "ok"], max_chars=10) == "ok"
    assert pack_prompt([]) == ""


def test_apply_vocabulary_fixes_casing_on_whole_words_only():
    out = apply_vocabulary("i use wordink and hyprland, not wordinks or hyprlandia", ["WordInk", "Hyprland"])
    assert out == "i use WordInk and Hyprland, not wordinks or hyprlandia"


def test_apply_vocabulary_handles_multiword_and_punctuation_terms():
    assert apply_vocabulary("open the c++ docs for open ai", ["C++", "Open AI"]) == "open the C++ docs for Open AI"


def _gateway(handler):
    return httpx.MockTransport(handler)


def test_load_vocabulary_merges_gateway_terms_after_local(tmp_path, monkeypatch):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    path = tmp_path / "v.txt"
    add_terms(path, ["WordInk"])

    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/v1/vocabulary"
        return httpx.Response(200, json={"object": "list", "data": [{"term": "wordink"}, {"term": "Tailscale"}]})

    terms = load_vocabulary(path, gateway="http://gw.test", use_gateway=True, transport=_gateway(handler))
    assert terms == ["WordInk", "Tailscale"]


def test_load_vocabulary_survives_gateway_failure(tmp_path, monkeypatch):
    monkeypatch.setenv("WORDINK_ADMIN_TOKEN", "wdk_admin")
    path = tmp_path / "v.txt"
    add_terms(path, ["WordInk"])
    down = _gateway(lambda r: httpx.Response(503, json={"error": {"message": "down"}}))
    assert load_vocabulary(path, gateway="http://gw.test", use_gateway=True, transport=down) == ["WordInk"]


def test_load_vocabulary_skips_gateway_when_not_requested(tmp_path):
    path = tmp_path / "v.txt"
    add_terms(path, ["WordInk"])

    def boom(request: httpx.Request) -> httpx.Response:
        raise AssertionError("gateway must not be called")

    assert load_vocabulary(path, transport=_gateway(boom)) == ["WordInk"]
