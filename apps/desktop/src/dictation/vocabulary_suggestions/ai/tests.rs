use std::sync::Mutex;

use super::*;
use crate::dictation::vocabulary_suggestions::Reason;
use crate::history::HistoryKind;

/// A fake provider: canned keys and reply, recording what was sent.
struct Fake {
    keys: Vec<Provider>,
    reply: Result<String, String>,
    sent: Mutex<Vec<(String, String, String)>>,
}

impl Fake {
    fn new(keys: &[Provider], reply: Result<String, String>) -> Self {
        Self {
            keys: keys.to_vec(),
            reply,
            sent: Mutex::new(Vec::new()),
        }
    }
}

impl Net for Fake {
    fn key(&self, provider: Provider) -> Option<String> {
        self.keys
            .contains(&provider)
            .then(|| "test-key".to_string())
    }

    fn post_json(&self, url: &str, key: &str, body: &str) -> Result<String, String> {
        self.sent
            .lock()
            .unwrap()
            .push((url.into(), key.into(), body.into()));
        self.reply.clone()
    }
}

fn chat(content: &str) -> String {
    json!({"choices": [{"message": {"content": content}}]}).to_string()
}

fn entry(text: &str) -> HistoryEntry {
    HistoryEntry {
        id: 0,
        timestamp_ms: 0,
        kind: HistoryKind::Dictation,
        raw_text: text.into(),
        final_text: text.into(),
        application: None,
        processing: None,
        audio_ms: 0,
        inference_ms: 0,
        total_ms: 0,
        usage: None,
    }
}

fn term(word: &str) -> VocabularyEntry {
    VocabularyEntry {
        term: word.into(),
        sounds_like: Vec::new(),
    }
}

fn words(guesses: &[Suggestion]) -> Vec<&str> {
    guesses.iter().map(|g| g.word.as_str()).collect()
}

#[test]
fn valid_json_becomes_labelled_guesses() {
    let content = r#"[{"word":"Omarchy","misheard_as":"Omarky","reason":"next to Hyprland"},
                       {"word":"Hyprland"}]"#;
    let guesses = parse_guesses(content, &[], &[]);
    assert_eq!(words(&guesses), ["Omarchy", "Hyprland"]);
    assert_eq!(
        guesses[0].reason,
        Reason::Ai {
            misheard_as: Some("Omarky".into()),
            reason: "next to Hyprland".into()
        }
    );
    assert!(guesses[0].sounds_like.is_empty());
}

#[test]
fn junk_yields_nothing_and_bad_items_are_skipped() {
    assert!(parse_guesses("I could not find any.", &[], &[]).is_empty());
    assert!(parse_guesses("", &[], &[]).is_empty());
    assert!(parse_guesses("{\"words\": 4}", &[], &[]).is_empty());
    assert!(parse_guesses("42", &[], &[]).is_empty());
    let mixed = r#"[1, null, {"reason":"no word"}, {"word":""}, {"word":"..."},
                    {"word":"Zorp","misheard_as":7,"reason":null}, "Dax"]"#;
    let guesses = parse_guesses(mixed, &[], &[]);
    assert_eq!(words(&guesses), ["Zorp", "Dax"]);
    let long = format!(r#"[{{"word":"{}"}}]"#, "x".repeat(65));
    assert!(parse_guesses(&long, &[], &[]).is_empty());
}

#[test]
fn code_fenced_json_and_talk_around_it_parse() {
    let fenced = "```json\n[{\"word\":\"Omarchy\",\"reason\":\"jargon\"}]\n```";
    assert_eq!(words(&parse_guesses(fenced, &[], &[])), ["Omarchy"]);
    let talk = "Sure! Here you go:\n[{\"word\":\"Zorp\"}]\nHope that helps.";
    assert_eq!(words(&parse_guesses(talk, &[], &[])), ["Zorp"]);
    let wrapped = r#"{"suggestions":[{"word":"Dax"}]}"#;
    assert_eq!(words(&parse_guesses(wrapped, &[], &[])), ["Dax"]);
}

#[test]
fn duplicates_known_and_dismissed_words_are_dropped_and_ten_is_the_cap() {
    let content = r#"[{"word":"Omarchy"},{"word":"omarchy"},{"word":"Rune"},
                       {"word":"Dax"},{"word":"Zorp"}]"#;
    let guesses = parse_guesses(content, &[term("rune")], &["DAX".to_string()]);
    assert_eq!(words(&guesses), ["Omarchy", "Zorp"]);
    let many: Vec<String> = (0..25)
        .map(|i| format!("{{\"word\":\"Word{i}\"}}"))
        .collect();
    let capped = parse_guesses(&format!("[{}]", many.join(",")), &[], &[]);
    assert_eq!(capped.len(), MAX_GUESSES);
}

#[test]
fn the_same_spelling_as_misheard_is_not_kept() {
    let guesses = parse_guesses(r#"[{"word":"Zorp","misheard_as":"zorp"}]"#, &[], &[]);
    assert_eq!(
        guesses[0].reason,
        Reason::Ai {
            misheard_as: None,
            reason: String::new()
        }
    );
}

#[test]
fn the_sample_is_final_text_only_and_capped() {
    let history: Vec<HistoryEntry> = (0..10)
        .map(|index| {
            let mut entry = entry(&"a".repeat(900));
            entry.raw_text = format!("raw-only-{index}");
            entry
        })
        .collect();
    let sample = transcript_sample(&history);
    assert!(sample.chars().count() <= MAX_TRANSCRIPT_CHARS + 10);
    assert!(!sample.contains("raw-only"));
    assert!(sample.chars().count() > MAX_TRANSCRIPT_CHARS - 10);
    assert_eq!(transcript_sample(&[entry("  ")]), "");
}

#[test]
fn provider_choice_prefers_groq_then_openrouter() {
    assert_eq!(pick_provider(|_| true), Some(Provider::Groq));
    assert_eq!(
        pick_provider(|provider| provider == Provider::OpenRouter),
        Some(Provider::OpenRouter)
    );
    assert_eq!(pick_provider(|provider| provider == Provider::OpenAi), None);
}

#[test]
fn suggest_sends_transcripts_and_vocabulary_to_groq_through_the_fake() {
    let net = Fake::new(
        &[Provider::Groq],
        Ok(chat(
            "[{\"word\":\"Omarchy\",\"misheard_as\":\"Omarky\",\"reason\":\"x\"}]",
        )),
    );
    let history = [entry("I run Omarky with Hyprland")];
    let guesses = suggest(&net, Provider::Groq, &history, &[term("Hyprland")], &[]).unwrap();
    assert_eq!(words(&guesses), ["Omarchy"]);
    let sent = net.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let (url, key, body) = &sent[0];
    assert_eq!(url, "https://api.groq.com/openai/v1/chat/completions");
    assert_eq!(key, "test-key");
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["model"], "llama-3.1-8b-instant");
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(user.contains("I run Omarky with Hyprland"));
    assert!(user.contains("Hyprland"));
}

#[test]
fn suggest_uses_openrouter_with_its_own_model() {
    let net = Fake::new(&[Provider::OpenRouter], Ok(chat("[]")));
    let guesses = suggest(&net, Provider::OpenRouter, &[entry("hello")], &[], &[]).unwrap();
    assert!(guesses.is_empty());
    let sent = net.sent.lock().unwrap();
    assert_eq!(sent[0].0, openrouter::CHAT_URL);
    assert!(sent[0].2.contains(OPENROUTER_MODEL));
}

#[test]
fn suggest_fails_without_a_key_and_sends_nothing() {
    let net = Fake::new(&[], Ok(chat("[]")));
    let error = suggest(&net, Provider::Groq, &[entry("hello")], &[], &[]).unwrap_err();
    assert!(error.contains("Groq"));
    assert!(net.sent.lock().unwrap().is_empty());
}

#[test]
fn suggest_surfaces_failures_and_unreadable_replies() {
    let down = Fake::new(&[Provider::Groq], Err("the connection failed".into()));
    assert_eq!(
        suggest(&down, Provider::Groq, &[entry("hello")], &[], &[]).unwrap_err(),
        "the connection failed"
    );
    let odd = Fake::new(&[Provider::Groq], Ok("not json".into()));
    assert!(suggest(&odd, Provider::Groq, &[entry("hello")], &[], &[]).is_err());
}

#[test]
fn suggest_with_no_transcripts_sends_nothing() {
    let net = Fake::new(&[Provider::Groq], Ok(chat("[]")));
    assert!(
        suggest(&net, Provider::Groq, &[], &[], &[])
            .unwrap()
            .is_empty()
    );
    assert!(net.sent.lock().unwrap().is_empty());
}
