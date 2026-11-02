//! Signal 4, opt-in: ask a small chat model which words in recent
//! transcripts the speech model probably got wrong. The model's answer is a
//! guess from context, parsed defensively and labelled "AI guess" where it
//! is shown. All network and key access goes through [`Net`], so tests use a
//! fake and never call a provider.

use serde_json::{Value, json};

use super::{Suggestion, filter_known};
use crate::dictation::vocabulary::VocabularyEntry;
use crate::history::HistoryEntry;
use crate::transcription::cloud::{Provider, openrouter};

/// The transcript text sent, in characters, across all entries.
pub const MAX_TRANSCRIPT_CHARS: usize = 4_000;
/// The most guesses kept.
pub const MAX_GUESSES: usize = 10;
const GROQ_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_MODEL: &str = "llama-3.1-8b-instant";
const OPENROUTER_MODEL: &str = "meta-llama/llama-3.1-8b-instruct";
const MAX_WORD_CHARS: usize = 64;
const MAX_REASON_CHARS: usize = 160;
const MAX_VOCABULARY_SENT: usize = 200;
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Keys and the one HTTP call this feature makes.
pub trait Net: Send + Sync {
    /// The key for `provider`, when there is one.
    fn key(&self, provider: Provider) -> Option<String>;
    /// POSTs `body` as JSON to `url` with a bearer key; the reply's body, or
    /// a reason without the URL or the key.
    fn post_json(&self, url: &str, key: &str, body: &str) -> Result<String, String>;
}

/// The real thing: the app's key store and its HTTP client.
pub struct Live;

impl Net for Live {
    fn key(&self, provider: Provider) -> Option<String> {
        crate::keys::resolve(provider.id(), provider.key_env()).map(|(key, _)| key)
    }

    fn post_json(&self, url: &str, key: &str, body: &str) -> Result<String, String> {
        let agent = crate::net::agent(url, |config| config.timeout_global(Some(TIMEOUT)));
        let mut response = agent
            .post(url)
            .header("Authorization", &format!("Bearer {key}"))
            .header("Content-Type", "application/json")
            .send(body.as_bytes())
            .map_err(|error| crate::net::reason(&error))?;
        response
            .body_mut()
            .read_to_string()
            .map_err(|_| "the reply could not be read".to_string())
    }
}

/// The provider AI suggestions use: Groq, else OpenRouter, whichever has a
/// key.
pub fn pick_provider(has_key: impl Fn(Provider) -> bool) -> Option<Provider> {
    [Provider::Groq, Provider::OpenRouter]
        .into_iter()
        .find(|provider| has_key(*provider))
}

/// The chat endpoint and the cheap small model for `provider`.
fn endpoint(provider: Provider) -> Option<(&'static str, &'static str)> {
    match provider {
        Provider::Groq => Some((GROQ_URL, GROQ_MODEL)),
        Provider::OpenRouter => Some((openrouter::CHAT_URL, OPENROUTER_MODEL)),
        _ => None,
    }
}

/// The newest transcripts' `final_text`, newest first, one per line, up to
/// [`MAX_TRANSCRIPT_CHARS`] characters in all. An entry that doesn't fit is
/// cut; later ones are left out.
pub fn transcript_sample(history: &[HistoryEntry]) -> String {
    let mut sample = String::new();
    let mut used = 0;
    for entry in history {
        let text = entry.final_text.trim();
        if text.is_empty() {
            continue;
        }
        let room = MAX_TRANSCRIPT_CHARS.saturating_sub(used);
        if room == 0 {
            break;
        }
        let piece: String = text.replace('\n', " ").chars().take(room).collect();
        used += piece.chars().count() + 1;
        sample.push_str(&piece);
        sample.push('\n');
    }
    sample
}

const INSTRUCTION: &str = "You review speech-to-text transcripts. Find up to 10 names, product names or jargon that the speech model probably spelled wrong or keeps getting wrong, judging from context. Reply with only a JSON array of objects: {\"word\": the correct spelling, \"misheard_as\": how the transcript spelled it (optional), \"reason\": a short reason}. Do not include words already in the vocabulary list. Reply [] when unsure.";

/// The chat request body: the instruction, the vocabulary, the transcripts.
pub fn request_body(model: &str, transcripts: &str, vocabulary: &[VocabularyEntry]) -> Value {
    let terms: Vec<&str> = vocabulary
        .iter()
        .take(MAX_VOCABULARY_SENT)
        .map(|entry| entry.term.as_str())
        .collect();
    json!({
        "model": model,
        "temperature": 0,
        "max_tokens": 600,
        "messages": [
            {"role": "system", "content": INSTRUCTION},
            {"role": "user", "content": format!(
                "Vocabulary already saved: {}\n\nTranscripts:\n{}",
                terms.join(", "),
                transcripts
            )},
        ],
    })
}

fn clip(text: &str, limit: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

/// The JSON inside a model's text: bare, in a code fence, or after a line of
/// talk. An array, or an object holding one.
fn extract_json(content: &str) -> Option<Value> {
    let content = content.trim();
    let mut tries: Vec<&str> = vec![content];
    if let (Some(open), Some(close)) = (content.find('['), content.rfind(']'))
        && open < close
    {
        tries.push(&content[open..=close]);
    }
    if let (Some(open), Some(close)) = (content.find('{'), content.rfind('}'))
        && open < close
    {
        tries.push(&content[open..=close]);
    }
    tries
        .into_iter()
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .find_map(|value| match value {
            Value::Array(_) => Some(value),
            Value::Object(object) => object.into_values().find(Value::is_array),
            _ => None,
        })
}

/// A model's text answer as guesses: junk entries are dropped, as are words
/// already in the Vocabulary or dismissed and repeats; at most
/// [`MAX_GUESSES`].
pub fn parse_guesses(
    content: &str,
    vocabulary: &[VocabularyEntry],
    dismissed: &[String],
) -> Vec<Suggestion> {
    let Some(Value::Array(items)) = extract_json(content) else {
        return Vec::new();
    };
    let guesses = items
        .iter()
        .filter_map(|item| {
            let (word, misheard, reason) = match item {
                Value::String(word) => (word.as_str(), None, None),
                Value::Object(object) => {
                    let text = |key: &str| object.get(key).and_then(Value::as_str);
                    (text("word")?, text("misheard_as"), text("reason"))
                }
                _ => return None,
            };
            let word = clip(word, MAX_WORD_CHARS + 1);
            if word.is_empty()
                || word.chars().count() > MAX_WORD_CHARS
                || !word.chars().any(char::is_alphanumeric)
            {
                return None;
            }
            let misheard_as = misheard
                .map(|text| clip(text, MAX_WORD_CHARS))
                .filter(|text| !text.is_empty() && text.to_lowercase() != word.to_lowercase());
            Some(Suggestion {
                word,
                sounds_like: Vec::new(),
                reason: super::Reason::Ai {
                    misheard_as,
                    reason: reason
                        .map(|text| clip(text, MAX_REASON_CHARS))
                        .unwrap_or_default(),
                },
            })
        })
        .collect();
    let mut guesses = filter_known(guesses, vocabulary, dismissed);
    guesses.truncate(MAX_GUESSES);
    guesses
}

/// A chat completion's reply text (`choices[0].message.content`).
fn reply_content(reply: &str) -> Option<String> {
    let reply: Value = serde_json::from_str(reply).ok()?;
    reply
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Asks `provider`'s small model for guesses from `history` (newest first),
/// sending the sample of `final_text` and the Vocabulary's terms.
pub fn suggest(
    net: &dyn Net,
    provider: Provider,
    history: &[HistoryEntry],
    vocabulary: &[VocabularyEntry],
    dismissed: &[String],
) -> Result<Vec<Suggestion>, String> {
    let (url, model) = endpoint(provider).ok_or("this provider can't review transcripts")?;
    let key = net
        .key(provider)
        .ok_or_else(|| format!("no {} key", provider.name()))?;
    let transcripts = transcript_sample(history);
    if transcripts.trim().is_empty() {
        return Ok(Vec::new());
    }
    let body = request_body(model, &transcripts, vocabulary).to_string();
    let reply = net.post_json(url, &key, &body)?;
    let content = reply_content(&reply).ok_or("the reply wasn't understood")?;
    Ok(parse_guesses(&content, vocabulary, dismissed))
}

#[cfg(test)]
mod tests;
