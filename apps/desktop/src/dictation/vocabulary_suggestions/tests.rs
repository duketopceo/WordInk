use super::*;
use crate::history::HistoryKind;

fn rule(phrase: &str, output: &str) -> TextReplacement {
    TextReplacement {
        matched_phrase: phrase.into(),
        output: output.into(),
    }
}

fn entry(raw: &str, final_text: &str) -> HistoryEntry {
    HistoryEntry {
        id: 0,
        timestamp_ms: 0,
        kind: HistoryKind::Dictation,
        raw_text: raw.into(),
        final_text: final_text.into(),
        application: None,
        processing: None,
        audio_ms: 0,
        inference_ms: 0,
        total_ms: 0,
        usage: None,
    }
}

fn texts(texts: &[&str]) -> Vec<HistoryEntry> {
    texts.iter().map(|text| entry(text, text)).collect()
}

fn term(word: &str) -> VocabularyEntry {
    VocabularyEntry {
        term: word.into(),
        sounds_like: Vec::new(),
    }
}

fn words(suggestions: &[Suggestion]) -> Vec<&str> {
    suggestions.iter().map(|s| s.word.as_str()).collect()
}

#[test]
fn a_replacements_output_is_proposed_with_its_phrase_as_sounds_like() {
    let rules = [
        rule("omarky", "Omarchy"),
        rule("omarkie", "omarchy"),
        rule("open code", "OpenCode"),
        rule("sig", "Best regards,\nLuke"),
        rule("um", ""),
        rule("at sign", "@"),
    ];
    let found = from_replacements(&rules, &[term("opencode")]);
    assert_eq!(words(&found), ["Omarchy"]);
    assert_eq!(found[0].sounds_like, ["omarky", "omarkie"]);
    assert_eq!(found[0].reason, Reason::Replacement { fired: 0 });
}

#[test]
fn a_replacement_that_only_recases_gets_no_sounds_like() {
    let found = from_replacements(&[rule("rune", "Rune")], &[]);
    assert_eq!(words(&found), ["Rune"]);
    assert!(found[0].sounds_like.is_empty());
}

#[test]
fn history_counts_the_times_a_replacement_fired() {
    let rules = [rule("omarky", "Omarchy"), rule("dax", "Dax")];
    let history = [
        entry("I run omarky today", "I run Omarchy today"),
        entry("omarky and Omarky again", "Omarchy and Omarchy again"),
        // Phrase inside a longer word does not count.
        entry("omarkys are odd", "omarkys are odd"),
        // Unchanged text never counts.
        entry("Omarchy", "Omarchy"),
    ];
    let counts = replacement_firings(&rules, &history);
    assert_eq!(counts.get("omarchy"), Some(&3));
    assert_eq!(counts.get("dax"), None);
}

#[test]
fn firing_counts_rank_replacement_proposals() {
    let rules = [rule("dax", "Dax"), rule("omarky", "Omarchy")];
    let history = [
        entry("omarky", "Omarchy"),
        entry("omarky", "Omarchy"),
        entry("dax", "Dax"),
    ];
    let found = local(&rules, &history, &[], &[]);
    assert_eq!(words(&found), ["Omarchy", "Dax"]);
    assert_eq!(found[0].reason, Reason::Replacement { fired: 2 });
    assert_eq!(found[1].reason, Reason::Replacement { fired: 1 });
}

#[test]
fn recurring_unusual_words_need_three_sightings() {
    let history = texts(&[
        "We shipped Hyprland today and the OpenCode build.",
        "Then Hyprland crashed, so I filed a note about OpenCode.",
        "Check Hyprland again; the NASA demo used OpenCode too.",
        "NASA wants it in v2.1 and GPT-4 and example.com.",
    ]);
    let found = recurring_words(&history, &[]);
    assert_eq!(words(&found), ["Hyprland", "OpenCode"]);
    assert_eq!(found[0].reason, Reason::Recurring { count: 3 });
}

#[test]
fn recurring_words_cover_each_shape() {
    let sentence = "Use node.js with GPT-4 and macOS plus ZFS and v2 now. ";
    let history = texts(&[sentence, sentence, sentence]);
    let mut found = words(&recurring_words(&history, &[])).join(" ");
    found.make_ascii_lowercase();
    for expected in ["node.js", "gpt-4", "macos", "zfs", "v2"] {
        assert!(found.contains(expected), "{expected} missing from {found}");
    }
}

#[test]
fn sentence_initial_capitals_and_common_words_are_not_proposed() {
    let history = texts(&[
        "Today I think the 2nd one is fine. On Monday at 5pm we met.",
        "Today I think the 3rd one is fine. On Monday at 5pm we met.",
        "Today I think the 4th one is fine. On Monday at 5pm we met. OK",
        "Well-known follow-up, e.g. this. Today",
        "Well-known follow-up, e.g. this. Today",
        "Well-known follow-up, e.g. this. Today",
    ]);
    assert_eq!(words(&recurring_words(&history, &[])), Vec::<&str>::new());
}

#[test]
fn a_capitalised_word_counts_only_where_it_is_not_sentence_initial() {
    // Twice mid-sentence and twice at a sentence start is two sightings.
    let history = texts(&[
        "Ask Zorp about it.",
        "Please ask Zorp.",
        "Zorp said yes.",
        "Right. Zorp agreed.",
    ]);
    assert!(recurring_words(&history, &[]).is_empty());
    let more = texts(&["Then ask Zorp"]);
    let mut all = history;
    all.extend(more);
    assert_eq!(words(&recurring_words(&all, &[])), ["Zorp"]);
}

#[test]
fn recurring_words_in_the_vocabulary_are_left_out_in_any_case() {
    let history = texts(&["ask Zorp", "ask Zorp", "ask Zorp"]);
    assert_eq!(words(&recurring_words(&history, &[])), ["Zorp"]);
    assert!(recurring_words(&history, &[term("zorp")]).is_empty());
}

#[test]
fn possessives_count_with_the_word() {
    let history = texts(&["ask Zorp", "so Zorp's idea? ask Zorp", "see Zorp’s plan"]);
    let found = recurring_words(&history, &[]);
    assert_eq!(words(&found), ["Zorp"]);
    assert_eq!(found[0].reason, Reason::Recurring { count: 4 });
}

#[test]
fn dismissed_and_known_words_are_filtered_in_any_case() {
    let rules = [rule("omarky", "Omarchy"), rule("dax", "Dax")];
    let history = texts(&["ask Zorp", "ask Zorp", "ask Zorp"]);
    let all = local(&rules, &history, &[], &[]);
    assert_eq!(words(&all), ["Omarchy", "Dax", "Zorp"]);
    let dismissed = vec!["omarchy".to_string(), "ZORP".to_string()];
    let kept = local(&rules, &history, &[term("DAX")], &dismissed);
    assert!(kept.is_empty());
    assert!(is_known("Zorp", &[], &dismissed));
    assert!(!is_known("Other", &[], &dismissed));
}

#[test]
fn replacement_and_recurring_signals_merge_into_one_proposal() {
    let rules = [rule("zorp", "Zorp")];
    let history = texts(&["ask Zorp", "ask Zorp", "ask Zorp"]);
    let found = local(&rules, &history, &[], &[]);
    assert_eq!(words(&found), ["Zorp"]);
    assert!(matches!(found[0].reason, Reason::Replacement { .. }));
}

#[test]
fn dismissing_remembers_a_word_once_and_caps_the_list() {
    let mut list = Vec::new();
    dismiss(&mut list, "  Zorp ");
    dismiss(&mut list, "zorp");
    dismiss(&mut list, "   ");
    assert_eq!(list, ["Zorp"]);
    for index in 0..MAX_DISMISSED + 5 {
        dismiss(&mut list, &format!("word{index}"));
    }
    assert_eq!(list.len(), MAX_DISMISSED);
    assert_eq!(list.last().map(String::as_str), Some("word504"));
    assert!(!list.contains(&"Zorp".to_string()));
}
