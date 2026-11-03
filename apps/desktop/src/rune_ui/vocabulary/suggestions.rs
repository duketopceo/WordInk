//! The Vocabulary sheet's Suggestions: words WordInk proposes from the
//! person's replacements and History, and, on request, an AI guess. Each is
//! Added to the list or Dismissed by the person; nothing is added on its
//! own, because only they know what is right.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use rune::{El, TextEdit, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant};

use super::super::Ui;
use super::super::controls::{ButtonKind, DESCRIPTION, Paint, PushSize, button};
use super::super::lists::section_label;
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::dictation::vocabulary_suggestions::ai::{self, Live, Net};
use crate::dictation::vocabulary_suggestions::{self as model, Reason, Suggestion};
use crate::history::HistoryEntry;
use crate::transcription::cloud::Provider;

/// The most proposals shown at once; dismissing or adding one brings the
/// next.
const SHOWN: usize = 4;

/// Whether an AI provider has a key, found off the interface thread (the
/// key store may wait).
enum KeyState {
    Unchecked,
    Checking(Receiver<Option<Provider>>),
    Ready(Provider),
    Missing,
}

enum Run {
    Idle,
    Running(Receiver<Result<Vec<Suggestion>, String>>),
    Failed(String),
}

pub(in crate::rune_ui) struct Suggestions {
    /// From replacements and History, found when the sheet opens.
    local: Vec<Suggestion>,
    /// What "Suggest with AI" returned.
    guesses: Vec<Suggestion>,
    key: KeyState,
    run: Run,
    net: Arc<dyn Net>,
}

impl Default for Suggestions {
    fn default() -> Self {
        Self {
            local: Vec::new(),
            guesses: Vec::new(),
            key: KeyState::Unchecked,
            run: Run::Idle,
            net: Arc::new(Live),
        }
    }
}

/// Every replacement the person has, in the Global mode and each mode.
fn replacements(ui: &Ui) -> Vec<crate::app_settings::TextReplacement> {
    let processing = &ui.settings.dictation_processing;
    processing
        .default_mode
        .replacements
        .iter()
        .chain(processing.modes.iter().flat_map(|mode| &mode.replacements))
        .cloned()
        .collect()
}

/// The retained dictations, newest first.
fn history(ui: &Ui) -> Vec<HistoryEntry> {
    match ui.history.store() {
        Some(store) => store.search(""),
        None => ui.history.entries.clone(),
    }
}

/// Finds the local proposals and, once, whether an AI provider has a key.
pub(super) fn refresh(ui: &mut Ui) {
    ui.vocabulary.suggestions.local = model::local(
        &replacements(ui),
        &history(ui),
        &ui.settings.vocabulary,
        &ui.settings.vocabulary_dismissed,
    );
    ui.vocabulary.suggestions.guesses.clear();
    ui.vocabulary.suggestions.run = Run::Idle;
    if matches!(ui.vocabulary.suggestions.key, KeyState::Unchecked) {
        ui.vocabulary.suggestions.key = if cfg!(test)
            || matches!(ui.service, super::super::Service::Fixture(_))
        {
            KeyState::Missing
        } else {
            let (sender, receiver) = std::sync::mpsc::channel();
            let net = ui.vocabulary.suggestions.net.clone();
            let spawned = std::thread::Builder::new()
                .name("vocabulary-key".into())
                .spawn(move || {
                    let _ = sender.send(ai::pick_provider(|provider| net.key(provider).is_some()));
                });
            match spawned {
                Ok(_) => KeyState::Checking(receiver),
                Err(_) => KeyState::Missing,
            }
        };
    }
}

/// Adopts a finished key check or AI request. True when anything changed.
pub(in crate::rune_ui) fn poll(ui: &mut Ui) -> bool {
    let suggestions = &mut ui.vocabulary.suggestions;
    let mut changed = false;
    if let KeyState::Checking(receiver) = &suggestions.key {
        match receiver.try_recv() {
            Ok(provider) => {
                suggestions.key = provider.map_or(KeyState::Missing, KeyState::Ready);
                changed = true;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                suggestions.key = KeyState::Missing;
                changed = true;
            }
        }
    }
    if let Run::Running(receiver) = &suggestions.run {
        let answer = match receiver.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the request stopped".into())),
        };
        if let Some(answer) = answer {
            match answer {
                Ok(guesses) => {
                    suggestions.guesses = guesses;
                    suggestions.run = Run::Idle;
                }
                Err(reason) => suggestions.run = Run::Failed(reason),
            }
            changed = true;
        }
    }
    changed
}

/// Suggest with AI: sends the recent transcripts and the list to the
/// provider, off the interface thread.
fn suggest_with_ai(ui: &mut Ui) {
    let KeyState::Ready(provider) = ui.vocabulary.suggestions.key else {
        return;
    };
    if matches!(ui.vocabulary.suggestions.run, Run::Running(_)) {
        return;
    }
    let history = history(ui);
    let vocabulary = ui.drafted().vocabulary;
    let dismissed = ui.settings.vocabulary_dismissed.clone();
    let net = ui.vocabulary.suggestions.net.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("vocabulary-ai".into())
        .spawn(move || {
            let _ = sender.send(ai::suggest(
                net.as_ref(),
                provider,
                &history,
                &vocabulary,
                &dismissed,
            ));
        });
    ui.vocabulary.suggestions.run = match spawned {
        Ok(_) => Run::Running(receiver),
        Err(error) => Run::Failed(error.to_string()),
    };
}

/// The proposals to show: not in the list, not dismissed, not typed into a
/// row, strongest first, at most [`SHOWN`].
fn visible(ui: &Ui) -> Vec<&Suggestion> {
    let suggestions = &ui.vocabulary.suggestions;
    let mut shown: Vec<&Suggestion> = Vec::new();
    for suggestion in suggestions.local.iter().chain(&suggestions.guesses) {
        let typed = ui
            .vocabulary
            .rows
            .iter()
            .any(|row| row.term.text.trim().eq_ignore_ascii_case(&suggestion.word));
        if !typed
            && !model::is_known(
                &suggestion.word,
                &ui.settings.vocabulary,
                &ui.settings.vocabulary_dismissed,
            )
            && !shown
                .iter()
                .any(|other| other.word.to_lowercase() == suggestion.word.to_lowercase())
        {
            shown.push(suggestion);
        }
    }
    shown.truncate(SHOWN);
    shown
}

/// Add: the word joins the list (in the last row when that is blank, else a
/// new one) and is saved now.
fn add(ui: &mut Ui, word: &str, sounds_like: &[String], tx: &mut Tx) {
    for row in &mut ui.vocabulary.rows {
        row.term.focused = false;
        row.sounds_like.focused = false;
    }
    let spellings = sounds_like.join(", ");
    let blank =
        ui.vocabulary.rows.last_mut().filter(|row| {
            row.term.text.trim().is_empty() && row.sounds_like.text.trim().is_empty()
        });
    match blank {
        Some(row) => {
            row.term = TextEdit::new(word);
            row.sounds_like = TextEdit::new(spellings);
        }
        None => {
            let id = ui
                .vocabulary
                .push(TextEdit::new(word), TextEdit::new(spellings));
            ui.vocabulary.added = Some(id);
        }
    }
    ui.flush_drafts();
    tx.scroll_to_bottom("term-scroll");
}

/// Dismiss: the word is remembered and never proposed again.
fn dismiss(ui: &mut Ui, word: &str) {
    ui.update(|settings| model::dismiss(&mut settings.vocabulary_dismissed, word));
}

/// Why `suggestion` is proposed, in words.
fn reason_text(suggestion: &Suggestion) -> String {
    match &suggestion.reason {
        Reason::Replacement { fired } if *fired >= 2 => tr_fill(
            "Your replacements fixed this {} times; adding it to Vocabulary helps the model get it right the first time.",
            &fired.to_string(),
        ),
        Reason::Replacement { .. } => tr(
            "You fix this with a replacement; adding it to Vocabulary helps the model get it right the first time.",
        )
        .to_string(),
        Reason::Recurring { count } => tr_fill("Heard {} times", &count.to_string()),
        Reason::Ai {
            misheard_as: Some(heard),
            reason,
        } => tr_named(
            "AI guess: heard as “{heard}”. {reason}",
            &[("heard", heard), ("reason", &sentence_case(reason))],
        )
        .trim()
        .to_string(),
        Reason::Ai { reason, .. } if reason.is_empty() => tr("AI guess").to_string(),
        Reason::Ai { reason, .. } => tr_fill("AI guess: {}", reason),
    }
}

/// `text` with its first letter capitalised, as models often start a
/// reason in lower case.
fn sentence_case(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn suggestion_row(p: Paint, index: usize, suggestion: &Suggestion) -> El {
    let word = suggestion.word.clone();
    let spellings = suggestion.sounds_like.clone();
    let dismissed = suggestion.word.clone();
    row()
        .key(format!("suggestion{index}"))
        .gap(8.0)
        .center_y()
        .child(
            col()
                .key("suggestion-text")
                .gap(2.0)
                .grow(1.0)
                .shrink()
                .child(
                    text(suggestion.word.clone())
                        .key("suggestion-word")
                        .font(p.k.ty.body_medium)
                        .color(p.text()),
                )
                .child(
                    text(reason_text(suggestion))
                        .key("suggestion-reason")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                ),
        )
        .child(button(
            p,
            &format!("dismiss-suggestion{index}"),
            tr("Dismiss"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| dismiss(ui, &dismissed),
        ))
        .child(button(
            p,
            &format!("add-suggestion{index}"),
            tr("Add"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, tx| add(ui, &word, &spellings, tx),
        ))
}

/// The Suggestions block under the term list: its title and the AI button,
/// a line on what it is, then the proposals.
pub(super) fn section(ui: &Ui, p: Paint) -> El {
    let suggestions = &ui.vocabulary.suggestions;
    let (label, enabled) = match (&suggestions.key, &suggestions.run) {
        (_, Run::Running(_)) => (tr("Suggesting…"), false),
        (KeyState::Ready(_), _) => (tr("Suggest with AI"), true),
        _ => (tr("Suggest with AI"), false),
    };
    let ai_button = Button::new(label)
        .variant(ButtonVariant::Neutral)
        .disabled(!enabled)
        .on_press_tx(|ui: &mut Ui, _: &mut Tx| suggest_with_ai(ui))
        .build_push(p.k)
        .key("suggest-ai");
    let mut block = col()
        .key("suggestions")
        .gap(8.0)
        .child(
            row()
                .key("suggestions-heading")
                .center_y()
                .child(section_label(p, "suggestions-title", tr("Suggestions")))
                .child(rune::el().key("suggestions-spacer").grow(1.0))
                .child(ai_button),
        )
        .child(
            text(tr(
                "WordInk proposes these from your replacements and history. Nothing is added until you press Add.",
            ))
            .key("suggestions-description")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap(),
        );
    let ai_note = match (&suggestions.key, &suggestions.run) {
        (_, Run::Failed(reason)) => Some((
            tr_fill("Couldn't get suggestions: {}", reason),
            p.danger(),
            "suggestions-error",
        )),
        (KeyState::Missing, _) => Some((
            tr("Add a Groq or OpenRouter key to use this").to_string(),
            p.muted(),
            "suggestions-no-key",
        )),
        (KeyState::Ready(provider), _) => Some((
            tr_fill(
                "Suggest with AI sends your recent transcripts (up to about 4,000 characters) and this list to {}.",
                provider.name(),
            ),
            p.muted(),
            "suggestions-privacy",
        )),
        _ => None,
    };
    if let Some((copy, color, key)) = ai_note {
        block = block.child(text(copy).key(key).font(DESCRIPTION).color(color).wrap());
    }
    for (index, suggestion) in visible(ui).into_iter().enumerate() {
        block = block.child(suggestion_row(p, index, suggestion));
    }
    block
}

/// The sheet's proposals for the previews: one of each kind.
pub(super) fn fixture(ui: &mut Ui) {
    let suggestion = |word: &str, sounds_like: &[&str], reason| Suggestion {
        word: word.into(),
        sounds_like: sounds_like.iter().map(|text| (*text).into()).collect(),
        reason,
    };
    ui.vocabulary.suggestions.local = vec![
        suggestion("Omarchy", &["omarky"], Reason::Replacement { fired: 5 }),
        suggestion("Hyprland", &[], Reason::Recurring { count: 7 }),
    ];
    ui.vocabulary.suggestions.guesses = vec![suggestion(
        "Zorp",
        &[],
        Reason::Ai {
            misheard_as: Some("Zorb".into()),
            reason: "a name next to Dax".into(),
        },
    )];
}

/// Replaces the key store and HTTP client with `net`, as if `provider` had
/// a key, so tests never reach a real provider.
#[cfg(all(test, target_os = "linux"))]
pub(in crate::rune_ui) fn install_fake(ui: &mut Ui, net: Arc<dyn Net>, provider: Option<Provider>) {
    let suggestions = &mut ui.vocabulary.suggestions;
    suggestions.net = net;
    suggestions.key = provider.map_or(KeyState::Missing, KeyState::Ready);
}

/// Where the AI review stands, for tests.
#[cfg(all(test, target_os = "linux"))]
pub(in crate::rune_ui) fn ai_status(ui: &Ui) -> &'static str {
    let suggestions = &ui.vocabulary.suggestions;
    match (&suggestions.run, &suggestions.key) {
        (Run::Running(_), _) => "running",
        (Run::Failed(_), _) => "failed",
        (_, KeyState::Unchecked) => "unchecked",
        (_, KeyState::Checking(_)) => "checking",
        (_, KeyState::Ready(_)) => "ready",
        (_, KeyState::Missing) => "missing",
    }
}

/// The words the AI review returned, for tests.
#[cfg(all(test, target_os = "linux"))]
pub(in crate::rune_ui) fn guess_words(ui: &Ui) -> Vec<String> {
    ui.vocabulary
        .suggestions
        .guesses
        .iter()
        .map(|guess| guess.word.clone())
        .collect()
}
