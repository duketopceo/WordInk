//! Proposed Vocabulary additions. Nothing here knows what is right: it reads
//! three local signals and an optional AI guess, and the person's Add or
//! Dismiss is the ground truth. Every function is pure (the AI request goes
//! through the injectable [`ai::Net`]).
//!
//! Signals, strongest first:
//! 1. the right-hand side of a replacement, a spelling the person already
//!    confirmed;
//! 2. how often History shows a replacement firing (raw text differs from
//!    the inserted text);
//! 3. unusual words that recur in History (names, jargon, acronyms);
//! 4. an AI guess from recent transcripts, only when asked for.

pub(crate) mod ai;

use std::collections::HashMap;

use regex::RegexBuilder;

use super::text_replacements::TextReplacement;
use super::vocabulary::{self, VocabularyEntry};
use crate::history::HistoryEntry;

/// A word seen at least this often in History is proposed.
pub const MIN_RECURRENCES: u32 = 3;
/// The most recurring-word proposals one list holds.
const MAX_RECURRING: usize = 20;
/// A replacement's output longer than this (in words or characters) is a
/// snippet of text, not a term.
const MAX_OUTPUT_WORDS: usize = 3;
const MAX_OUTPUT_CHARS: usize = 40;
/// The most dismissed words kept; the oldest go first.
pub const MAX_DISMISSED: usize = 500;

/// Why a word is proposed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reason {
    /// The person's own replacement produces it. `fired` is how many times
    /// History shows it correcting a transcript (0 when never seen).
    Replacement { fired: u32 },
    /// It recurs in History: `count` times.
    Recurring { count: u32 },
    /// A guess from context by a chat model.
    Ai {
        misheard_as: Option<String>,
        reason: String,
    },
}

/// One proposed word.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    pub word: String,
    /// Spellings a transcript showed instead, which Add saves as "sounds
    /// like". Only a confirmed replacement fills them.
    pub sounds_like: Vec<String>,
    pub reason: Reason,
}

impl Suggestion {
    /// Where the proposal ranks: lower is stronger.
    fn rank(&self) -> (u8, std::cmp::Reverse<u32>) {
        match &self.reason {
            Reason::Replacement { fired } => (0, std::cmp::Reverse(*fired)),
            Reason::Recurring { count } => (1, std::cmp::Reverse(*count)),
            Reason::Ai { .. } => (2, std::cmp::Reverse(0)),
        }
    }
}

fn same(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `word` is already in the list or was dismissed.
pub fn is_known(word: &str, vocabulary: &[VocabularyEntry], dismissed: &[String]) -> bool {
    vocabulary::contains(vocabulary, word) || dismissed.iter().any(|gone| same(gone, word))
}

/// Drops what is already in the list, dismissed, or repeated (in any case),
/// keeping the first of each.
pub fn filter_known(
    suggestions: Vec<Suggestion>,
    vocabulary: &[VocabularyEntry],
    dismissed: &[String],
) -> Vec<Suggestion> {
    let mut kept: Vec<Suggestion> = Vec::new();
    for suggestion in suggestions {
        if !is_known(&suggestion.word, vocabulary, dismissed)
            && !kept.iter().any(|other| same(&other.word, &suggestion.word))
        {
            kept.push(suggestion);
        }
    }
    kept
}

/// A replacement's output as a proposable term: short, with a letter or
/// digit in it. Snippets, symbols and deletions (an empty output) are not.
fn term_of(rule: &TextReplacement) -> Option<String> {
    let output = clean(&rule.output);
    let usable = !output.is_empty()
        && !rule.output.trim().contains(['\n', '\r'])
        && output.chars().count() <= MAX_OUTPUT_CHARS
        && output.split(' ').count() <= MAX_OUTPUT_WORDS
        && output.chars().any(char::is_alphanumeric);
    usable.then_some(output)
}

/// Signal 1: the outputs of the person's replacements that the Vocabulary
/// lacks. The matched phrases become the proposal's "sounds like".
pub fn from_replacements(
    rules: &[TextReplacement],
    vocabulary: &[VocabularyEntry],
) -> Vec<Suggestion> {
    let mut found: Vec<Suggestion> = Vec::new();
    for rule in rules {
        let Some(term) = term_of(rule) else { continue };
        if vocabulary::contains(vocabulary, &term) {
            continue;
        }
        let heard = clean(&rule.matched_phrase);
        match found.iter_mut().find(|known| same(&known.word, &term)) {
            Some(known) => {
                if !heard.is_empty() && !known.sounds_like.iter().any(|kept| same(kept, &heard)) {
                    known.sounds_like.push(heard);
                }
            }
            None => found.push(Suggestion {
                word: term,
                sounds_like: if heard.is_empty() || same(&heard, &rule.output) {
                    Vec::new()
                } else {
                    vec![heard]
                },
                reason: Reason::Replacement { fired: 0 },
            }),
        }
    }
    found
}

/// Signal 2: how often each replacement output was produced by a rule that
/// fired in History: its phrase is in `raw_text` and the output is in
/// `final_text`. Keyed by the output in lowercase.
pub fn replacement_firings(
    rules: &[TextReplacement],
    history: &[HistoryEntry],
) -> HashMap<String, u32> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for rule in rules {
        let Some(term) = term_of(rule) else { continue };
        let phrase = clean(&rule.matched_phrase);
        if phrase.is_empty() || phrase == term {
            continue;
        }
        let Ok(matcher) = RegexBuilder::new(&format!(
            r"(?:^|[^\p{{L}}\p{{N}}])({})(?:$|[^\p{{L}}\p{{N}}])",
            regex::escape(&phrase)
        ))
        .case_insensitive(true)
        .build() else {
            continue;
        };
        let fired: u32 = history
            .iter()
            .filter(|entry| entry.raw_text != entry.final_text)
            .filter(|entry| entry.final_text.contains(&term))
            .map(|entry| {
                // A match already spelled as the output wasn't changed.
                let changed = matcher
                    .captures_iter(&entry.raw_text)
                    .filter(|found| found.get(1).is_some_and(|text| text.as_str() != term))
                    .count();
                u32::try_from(changed).unwrap_or(u32::MAX)
            })
            .fold(0, u32::saturating_add);
        if fired > 0 {
            let total = counts.entry(term.to_lowercase()).or_insert(0);
            *total = total.saturating_add(fired);
        }
    }
    counts
}

/// A tiny list of very common words that are capitalised or look unusual
/// without being names or jargon. Deliberately short.
const STOPLIST: &[&str] = &[
    "i",
    "i'm",
    "i'll",
    "i've",
    "i'd",
    "ok",
    "okay",
    "am",
    "pm",
    "tv",
    "us",
    "uk",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "e.g",
    "i.e",
    "a.m",
    "p.m",
    "etc",
    "vs",
];

/// One word of a transcript and whether a sentence starts with it.
struct Token {
    word: String,
    sentence_start: bool,
}

/// Splits `text` into words (letters, digits and inner `. - _ '`), noting
/// sentence starts after `. ! ? :` and line breaks.
fn tokens(text: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut start = true;
    let mut current = String::new();
    let mut flush = |current: &mut String, start: &mut bool| {
        if current.is_empty() {
            return;
        }
        let word = current.trim_matches(|c: char| !c.is_alphanumeric());
        if !word.is_empty() {
            found.push(Token {
                word: word.to_string(),
                sentence_start: *start,
            });
            *start = false;
        }
        if current.ends_with('.') {
            *start = true;
        }
        current.clear();
    };
    for character in text.chars().chain(std::iter::once(' ')) {
        if character.is_alphanumeric() || matches!(character, '.' | '-' | '_' | '\'' | '’') {
            current.push(character);
        } else {
            flush(&mut current, &mut start);
            if matches!(character, '!' | '?' | '\n' | ':') {
                start = true;
            }
        }
    }
    found
}

fn strip_possessive(word: &str) -> &str {
    word.strip_suffix("'s")
        .or_else(|| word.strip_suffix("’s"))
        .unwrap_or(word)
}

/// A number's ordinal or a clock time: digits with a common suffix.
fn is_counting_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    let letters = lower.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    letters.len() < lower.len() && matches!(letters, "st" | "nd" | "rd" | "th" | "am" | "pm" | "s")
}

/// Whether `word` looks like a name or jargon, for the recurring-word
/// signal: capitalised mid-sentence, CamelCase, digits with letters, an
/// inner dot (or a dash beside a capital or digit), or an ALL-CAPS acronym
/// of two letters or more.
fn looks_unusual(word: &str, sentence_start: bool) -> bool {
    let word = strip_possessive(word);
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    if letters == 0 || word.chars().count() < 2 || STOPLIST.contains(&word.to_lowercase().as_str())
    {
        return false;
    }
    let has_digit = word.chars().any(|c| c.is_ascii_digit());
    let has_upper = word.chars().any(char::is_uppercase);
    let has_lower = word.chars().any(char::is_lowercase);
    if has_digit {
        return !is_counting_word(word);
    }
    if has_upper && !has_lower && letters >= 2 && word.chars().all(char::is_alphabetic) {
        return true;
    }
    let mut chars = word.chars();
    let first_upper = chars.next().is_some_and(char::is_uppercase);
    if chars.any(char::is_uppercase) && has_lower {
        // CamelCase, iPhone, macOS; also a capital after an inner dash.
        return true;
    }
    // Trimmed words keep only inner dots: node.js, example.com.
    if word.contains('.') {
        return true;
    }
    first_upper && has_lower && !sentence_start
}

/// Signal 3: words from `final_text` that recur at least [`MIN_RECURRENCES`]
/// times, look unusual and aren't in the Vocabulary. Most frequent first.
pub fn recurring_words(
    history: &[HistoryEntry],
    vocabulary: &[VocabularyEntry],
) -> Vec<Suggestion> {
    // Lowercase key -> (spelling -> count)
    let mut seen: HashMap<String, HashMap<String, u32>> = HashMap::new();
    for entry in history {
        for token in tokens(&entry.final_text) {
            if !looks_unusual(&token.word, token.sentence_start) {
                continue;
            }
            let word = strip_possessive(&token.word).to_string();
            *seen
                .entry(word.to_lowercase())
                .or_default()
                .entry(word)
                .or_insert(0) += 1;
        }
    }
    let mut found: Vec<Suggestion> = seen
        .into_values()
        .filter_map(|spellings| {
            let count: u32 = spellings.values().sum();
            let (word, _) = spellings
                .into_iter()
                .max_by(|(left, a), (right, b)| a.cmp(b).then_with(|| right.cmp(left)))?;
            (count >= MIN_RECURRENCES && !vocabulary::contains(vocabulary, &word)).then_some(
                Suggestion {
                    word,
                    sounds_like: Vec::new(),
                    reason: Reason::Recurring { count },
                },
            )
        })
        .collect();
    found.sort_by(|left, right| {
        left.rank()
            .cmp(&right.rank())
            .then(left.word.cmp(&right.word))
    });
    found.truncate(MAX_RECURRING);
    found
}

/// Everything the three local signals propose, strongest first, without what
/// the Vocabulary has or the person dismissed.
pub fn local(
    rules: &[TextReplacement],
    history: &[HistoryEntry],
    vocabulary: &[VocabularyEntry],
    dismissed: &[String],
) -> Vec<Suggestion> {
    let firings = replacement_firings(rules, history);
    let mut all = from_replacements(rules, vocabulary);
    for suggestion in &mut all {
        if let Some(fired) = firings.get(&suggestion.word.to_lowercase()) {
            suggestion.reason = Reason::Replacement { fired: *fired };
        }
    }
    all.extend(recurring_words(history, vocabulary));
    all.sort_by_key(Suggestion::rank);
    filter_known(all, vocabulary, dismissed)
}

/// Adds `word` to the end of `dismissed`, once and within the cap.
pub fn dismiss(dismissed: &mut Vec<String>, word: &str) {
    let word = clean(word);
    if word.is_empty() || dismissed.iter().any(|gone| same(gone, &word)) {
        return;
    }
    dismissed.push(word);
    if dismissed.len() > MAX_DISMISSED {
        let excess = dismissed.len() - MAX_DISMISSED;
        dismissed.drain(..excess);
    }
}

#[cfg(test)]
mod tests;
