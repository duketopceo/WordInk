//! The History pane: retained dictations (newest first) with search, the
//! retention choice, two-step Clear all, and a detail view with Copy and
//! Delete. It shares the host's store, which the listener records into.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use rune::{El, TextEdit, Tx, col, text};
use rune_kit::{Select, SelectAppearance, TextInput};

use super::controls::{ButtonKind, Paint, button};
use super::list_detail::{self as ld, Item};
use super::lists::{error_panel, section_label};
use super::{Menu, Ui};
use crate::desktop::i18n::{event_age, tr, tr_fill, tr_named};
use crate::history::stats::HistoryStats;
use crate::history::{History, HistoryEntry, HistoryKind, HistoryRetention};
use crate::transcription::cloud::{self, Provider};
use crate::transcription::recover::{self, FailedClip, Retried, RetryError};

/// How often the open pane looks for newly kept clips.
const CLIP_SCAN: Duration = Duration::from_secs(2);

/// A retry in flight on its own thread.
pub(super) struct Retrying {
    pub(super) clip: String,
    pub(super) result: Receiver<Result<Retried, RetryError>>,
    /// A fixture sends nothing, so nothing answers; this keeps the channel
    /// open so the pane reads as waiting rather than stopped.
    #[allow(dead_code)] // held only to keep the channel open
    pub(super) fixture_sender: Option<std::sync::mpsc::Sender<Result<Retried, RetryError>>>,
}

/// What a successful retry recovered: shown with Copy and Paste, never
/// inserted by itself.
pub(super) struct Recovered {
    pub(super) text: String,
    pub(super) engine: String,
    /// Paste was pressed: the text is on the clipboard.
    pub(super) pasted: bool,
}

#[derive(Default)]
pub(super) struct HistoryPane {
    store: Option<History>,
    pub(super) entries: Vec<HistoryEntry>,
    search: TextEdit,
    /// The query the entries were loaded for.
    searched: String,
    pub(super) selected: Option<u64>,
    pub(super) clear_armed: bool,
    pub(super) copied: Option<u64>,
    pub(super) error: Option<String>,
    /// Labs › Dictation stats: every retained entry summed up, while on.
    pub(super) stats: Option<HistoryStats>,
    /// Kept clips of dictations that failed, newest first (R11).
    pub(super) failed: Vec<FailedClip>,
    clips_dir: Option<PathBuf>,
    clips_scanned: Option<Instant>,
    /// The failed dictation open in the detail, by file name.
    pub(super) selected_failed: Option<String>,
    /// The engine Retry sends the open clip to.
    pub(super) retry_engine: Option<Provider>,
    /// A local clip waiting for the person to say it may go to the cloud.
    pub(super) confirm: Option<String>,
    pub(super) retrying: Option<Retrying>,
    pub(super) recovered: Option<Recovered>,
    pub(super) retry_error: Option<String>,
}

impl HistoryPane {
    /// The host's store; `None` when it could not open one.
    pub(super) fn store(&self) -> Option<&History> {
        self.store.as_ref()
    }

    /// The host's store; `None` when it could not open one.
    pub(super) fn shared(store: Option<History>) -> Self {
        Self {
            clips_dir: cloud::failed_clips_dir(),
            error: store
                .is_none()
                .then(|| tr("History could not be loaded.").to_string()),
            store,
            ..Self::default()
        }
    }

    /// Refreshes the bounded snapshot for the current query, and the stats.
    pub(super) fn reload(&mut self) {
        self.reload_entries();
        self.reload_stats();
        self.reload_clips();
    }

    /// The kept clips of failed dictations.
    fn reload_clips(&mut self) {
        self.clips_scanned = Some(Instant::now());
        let Some(dir) = &self.clips_dir else {
            return;
        };
        self.failed = recover::list(dir);
        if self
            .selected_failed
            .as_ref()
            .is_some_and(|name| !self.failed.iter().any(|clip| clip.file_name() == *name))
        {
            self.selected_failed = None;
            self.confirm = None;
        }
    }

    /// A retry is running or, in a fixture, was requested.
    #[cfg(test)]
    pub(super) fn is_retrying(&self) -> bool {
        self.retrying.is_some()
    }

    /// The engine Retry offers for `clip`: the one that failed, when it was
    /// a cloud engine, else the one chosen, else Groq.
    pub(super) fn engine_for(&self, clip: &FailedClip) -> Provider {
        self.retry_engine
            .or_else(|| Provider::from_id(&clip.origin))
            .or_else(cloud::chosen_provider)
            .unwrap_or(Provider::Groq)
    }

    /// Adopts a finished retry.
    fn poll_retry(&mut self) -> bool {
        let Some(retrying) = &self.retrying else {
            return false;
        };
        let answer = match retrying.result.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err(RetryError::Failed("the retry stopped".into())),
        };
        let clip = self.retrying.take().map(|r| r.clip).unwrap_or_default();
        match answer {
            Ok(retried) => {
                let engine = format!("{} · {}", retried.usage.provider, retried.usage.model);
                self.recovered = Some(Recovered {
                    text: retried.text,
                    engine,
                    pasted: false,
                });
                self.selected = None;
                self.selected_failed = None;
                self.retry_error = None;
            }
            Err(RetryError::NeedsConfirmation) => self.confirm = Some(clip),
            Err(RetryError::Failed(message)) => self.retry_error = Some(message),
        }
        self.reload();
        true
    }

    fn reload_entries(&mut self) {
        self.searched = self.search.text.clone();
        let Some(store) = &self.store else {
            return;
        };
        self.entries = store.search(&self.searched);
        if self
            .selected
            .is_some_and(|id| !self.entries.iter().any(|entry| entry.id == id))
        {
            self.selected = None;
        }
    }

    /// Sums up every retained entry, only while Labs › Dictation stats is
    /// on; off, nothing is read for it.
    fn reload_stats(&mut self) {
        self.stats = match &self.store {
            Some(store) if crate::labs::history_stats() => Some(store.stats()),
            _ => None,
        };
    }

    /// While the pane shows, picks up new dictations and search edits. The
    /// stats are summed again only when the entries or the toggle changed.
    pub(super) fn poll(&mut self, visible: bool) -> bool {
        let retried = self.poll_retry();
        if !visible || self.store.is_none() {
            return retried;
        }
        let mut changed = retried;
        if self
            .clips_scanned
            .is_none_or(|at| at.elapsed() >= CLIP_SCAN)
        {
            let before = self.failed.clone();
            self.reload_clips();
            changed |= self.failed != before;
        }
        let previous = std::mem::take(&mut self.entries);
        self.reload_entries();
        changed |= self.entries != previous;
        if self.entries != previous || self.stats.is_some() != crate::labs::history_stats() {
            self.reload_stats();
            return true;
        }
        changed
    }

    fn entry(&self, id: u64) -> Option<&HistoryEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub(super) fn fixture() -> Self {
        let now = crate::history::now_ms();
        let entry =
            |id: u64, minutes: u64, kind, raw: &str, final_text: &str, app: Option<&str>| {
                HistoryEntry {
                    id,
                    timestamp_ms: now - minutes * 60_000 - 30_000,
                    kind,
                    raw_text: raw.into(),
                    final_text: final_text.into(),
                    application: app.map(str::to_owned),
                    processing: None,
                    audio_ms: 4200,
                    inference_ms: 180,
                    total_ms: 640,
                    usage: None,
                }
            };
        let mut rewritten = entry(
            2,
            42,
            HistoryKind::Dictation,
            "so um the deploy failed because the the cache key changed",
            "The deploy failed because the cache key changed.",
            Some("Slack"),
        );
        rewritten.processing = Some(crate::desktop::events::DictationProcessing {
            profile: "Chat".into(),
            latency_ms: 910,
            fallback: None,
        });
        Self {
            entries: vec![
                entry(
                    3,
                    2,
                    HistoryKind::Dictation,
                    "Let's move the standup to ten thirty tomorrow.",
                    "Let's move the standup to ten thirty tomorrow.",
                    Some("WezTerm"),
                ),
                rewritten,
                entry(
                    1,
                    180,
                    HistoryKind::Dictation,
                    "Thanks for sending this over! I'll review it today and follow up with comments.",
                    "Thanks for sending this over! I'll review it today and follow up with comments.",
                    Some("Firefox"),
                ),
            ],
            selected: Some(2),
            ..Self::default()
        }
    }

    pub(super) fn fixture_empty() -> Self {
        Self::default()
    }

    /// Two failed dictations beside a retained one: a Groq failure, and
    /// audio recorded for a local model.
    pub(super) fn fixture_failed() -> Self {
        let now = crate::history::now_ms();
        let clip = |minutes: u64, origin: &str| FailedClip {
            path: PathBuf::from(format!(
                "/fixture/{}",
                cloud::clip_name(now - minutes * 60_000, origin)
            )),
            timestamp_ms: now - minutes * 60_000,
            origin: origin.into(),
        };
        let failed = vec![clip(3, "groq"), clip(75, "local")];
        Self {
            selected_failed: Some(failed[0].file_name()),
            selected: None,
            failed,
            ..Self::fixture()
        }
    }

    /// A retry that recovered text, waiting for Copy or Paste.
    pub(super) fn fixture_recovered() -> Self {
        Self {
            recovered: Some(Recovered {
                text: "Please move the Thursday review to ten, and send the draft to Maya first."
                    .into(),
                engine: "openai · gpt-4o-mini-transcribe".into(),
                pasted: false,
            }),
            selected: None,
            ..Self::fixture_failed()
        }
    }
}

fn set_retention(ui: &mut Ui, retention: HistoryRetention) {
    ui.open = None;
    if ui.settings.history_retention == retention {
        return;
    }
    if !ui.update(|settings| settings.history_retention = retention) {
        return;
    }
    if let Some(store) = &ui.history.store
        && let Err(error) = store.set_retention(retention)
    {
        ui.history.error = Some(error.to_string());
    }
    ui.history.reload();
}

fn clear(ui: &mut Ui) {
    if !ui.history.clear_armed {
        ui.history.clear_armed = true;
        return;
    }
    ui.history.clear_armed = false;
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.clear() {
            ui.history.error = Some(error.to_string());
        }
        ui.history.reload();
    } else {
        ui.history.entries.clear();
        ui.history.selected = None;
    }
}

fn delete(ui: &mut Ui, id: u64) {
    if let Some(store) = &ui.history.store {
        if let Err(error) = store.delete(id) {
            ui.history.error = Some(error.to_string());
        }
        ui.history.reload();
    } else {
        ui.history.entries.retain(|entry| entry.id != id);
        ui.history.selected = None;
    }
}

/// The header actions (retention, Clear all) and the list + detail body.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    let pane = &ui.history;
    let actions = header_actions(ui, p, layer);
    let mut notices = Vec::new();
    if let Some(error) = &pane.error {
        notices.push(error_panel(
            p,
            "history-error",
            tr("History could not be loaded."),
            error,
        ));
    }
    if ui.settings.labs.history_stats
        && let Some(stats) = pane.stats.as_ref().filter(|stats| stats.dictations > 0)
    {
        notices.push(stats_block(p, stats));
    }
    let searching = !pane.searched.trim().is_empty();
    let body = match empty(ui, p).filter(|_| !searching) {
        Some(empty) => empty,
        None => {
            let mut list = col().key("history-list");
            for clip in &pane.failed {
                list = list.child(super::recovery::failed_item(p, pane, clip));
            }
            for entry in &pane.entries {
                let id = entry.id;
                let mut meta = tr(entry.kind.label()).to_string();
                if let Some(application) = &entry.application {
                    meta.push_str(" · ");
                    meta.push_str(application);
                }
                list = list.child(ld::item(
                    p,
                    Item {
                        key: format!("history-entry{id}"),
                        title: entry.final_text.replace('\n', " "),
                        meta,
                        meta_ink: None,
                        time: event_age(entry.timestamp_ms),
                        hover: vec![copy_action(ui, p, id, true)],
                        selected: pane.selected == Some(id),
                    },
                    move |ui: &mut Ui| {
                        ui.history.selected = Some(id);
                        ui.history.selected_failed = None;
                        ui.history.recovered = None;
                        ui.history.confirm = None;
                        ui.history.clear_armed = false;
                    },
                ));
            }
            let search = TextInput::new(&pane.search)
                .placeholder(tr("Search history"))
                .width(None)
                .leading_icon(p.k.glyph.search)
                .on_edit(|ui: &mut Ui| &mut ui.history.search)
                .build(p.k)
                .key("history-search");
            let column = ld::list_column(search, "history-scroll", list);
            ld::body(p, "history-body", column, detail(ui, p, layer))
        }
    };
    (
        actions,
        col()
            .key("history-pane")
            .grow(1.0)
            .shrink()
            .children((!notices.is_empty()).then(|| ld::notices(notices)))
            .child(body),
    )
}

/// Labs › Dictation stats: every retained dictation summed up, above the
/// list. Shown only while something is retained.
fn stats_block(p: Paint, stats: &HistoryStats) -> El {
    let wpm = stats
        .words_per_minute
        .map_or_else(|| "–".to_owned(), |wpm| wpm.to_string());
    let mut lines = vec![
        tr_named(
            "Dictations: {count} · Words: {words} · Words per minute: {wpm}",
            &[
                ("count", &stats.dictations.to_string()),
                ("words", &stats.words.to_string()),
                ("wpm", &wpm),
            ],
        ),
        tr_named(
            "Changed by Output: {percent}% · Daily streak: {days}",
            &[
                (
                    "percent",
                    &stats.changed_percent().unwrap_or_default().to_string(),
                ),
                ("days", &stats.streak_days.to_string()),
            ],
        ),
    ];
    if !stats.top_apps.is_empty() {
        let apps = stats
            .top_apps
            .iter()
            .map(|app| format!("{} ({})", app.application, app.dictations))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(tr_fill("Top apps: {}", &apps));
    }
    col()
        .key("history-stats")
        .gap(4.0)
        .px(12.0)
        .py(10.0)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .child(section_label(p, "label", tr("Dictation stats")))
        .children(lines.into_iter().enumerate().map(|(index, line)| {
            text(line)
                .key(format!("line{index}"))
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap()
        }))
}

/// Retention as a borderless menu, and Clear all beside it while there is
/// something to clear.
fn header_actions(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<El> {
    let labels: Vec<&str> = HistoryRetention::ALL
        .iter()
        .map(|r| tr(r.label()))
        .collect();
    let current = tr_fill("Keep: {}", tr(ui.settings.history_retention.label()));
    let retention = Select::new("history-retention", &labels)
        .appearance(SelectAppearance::Inline)
        .current(Some(&current))
        .open(ui.open == Some(Menu::HistoryRetention))
        .on_toggle(|ui: &mut Ui| {
            ui.open = (ui.open != Some(Menu::HistoryRetention)).then_some(Menu::HistoryRetention);
        })
        .on_close(|ui: &mut Ui| ui.open = None)
        .on_choose(|ui: &mut Ui, choice: &str, _: &mut Tx| {
            if let Some(retention) = HistoryRetention::ALL
                .into_iter()
                .find(|r| tr(r.label()) == choice)
            {
                set_retention(ui, retention);
            }
        })
        .build(p.k, layer);
    let mut actions = vec![retention];
    if !ui.history.entries.is_empty() || ui.history.clear_armed {
        actions.push(if ui.history.clear_armed {
            button(
                p,
                "history-clear",
                tr("Really clear all?"),
                ButtonKind::Danger,
                |ui: &mut Ui, _| clear(ui),
            )
        } else {
            button(
                p,
                "history-clear",
                tr("Clear all"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| clear(ui),
            )
        });
    }
    actions
}

/// Copy as an icon: a row's hover action, or the card's.
fn copy_action(ui: &Ui, p: Paint, id: u64, in_row: bool) -> El {
    let glyph = if ui.history.copied == Some(id) {
        p.k.glyph.check_small
    } else {
        ld::COPY_ICON
    };
    let press = move |ui: &mut Ui, tx: &mut Tx| {
        if let Some(entry) = ui.history.entry(id) {
            tx.copy(entry.final_text.clone());
            ui.history.copied = Some(id);
            ui.history.error = None;
        }
    };
    if in_row {
        ld::row_action(p, "row-copy", glyph, tr("Copy"), press)
    } else {
        ld::ghost_icon(p, "history-copy", glyph, tr("Copy"), press)
    }
}

/// Nothing retained, or nothing matching: what History keeps and for how
/// long.
fn empty(ui: &Ui, p: Paint) -> Option<El> {
    let pane = &ui.history;
    if !pane.entries.is_empty() || !pane.failed.is_empty() {
        return None;
    }
    let retention = ui.settings.history_retention;
    Some(if !pane.searched.trim().is_empty() {
        ld::empty_state(
            p,
            "history-empty",
            ld::SEARCH_ICON,
            tr("No dictations match."),
            None,
        )
    } else if retention.is_off() {
        ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("History is off."),
            Some(tr("Choose how long to keep dictations to see them here.")),
        )
    } else {
        let hint = match retention {
            HistoryRetention::Forever => tr("Everything you dictate is kept here.").to_string(),
            other => tr_fill(
                "Everything you dictate is kept here for {}.",
                tr(other.label()),
            ),
        };
        ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("No dictations retained yet."),
            Some(&hint),
        )
    })
}

fn detail(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let pane = &ui.history;
    if let Some(recovered) = &pane.recovered {
        return super::recovery::recovered_detail(p, recovered);
    }
    if let Some(clip) = pane
        .selected_failed
        .as_ref()
        .and_then(|name| pane.failed.iter().find(|clip| clip.file_name() == *name))
    {
        return super::recovery::failed_detail(ui, p, clip, layer);
    }
    let Some(entry) = pane.selected.and_then(|id| pane.entry(id)) else {
        return empty(ui, p).unwrap_or_else(|| {
            ld::empty_state(
                p,
                "history-detail",
                ld::HISTORY_ICON,
                tr("Select a history entry."),
                None,
            )
        });
    };
    let id = entry.id;
    let mut latency = tr_named(
        "{audio} ms audio · {inference} ms inference",
        &[
            ("audio", &entry.audio_ms.to_string()),
            ("inference", &entry.inference_ms.to_string()),
        ],
    );
    if entry.total_ms > 0 {
        latency.push_str(&format!(
            " · {}",
            tr_fill("{} ms total", &entry.total_ms.to_string())
        ));
    }
    let mut facts = Vec::new();
    if let Some(application) = &entry.application {
        facts.push((tr("Application"), application.clone()));
    }
    if let Some(processing) = &entry.processing {
        let mut summary = format!("{} · {} ms", processing.profile, processing.latency_ms);
        if let Some(fallback) = &processing.fallback {
            summary.push_str(&format!(" · {}", tr_fill("fell back: {}", fallback)));
        }
        facts.push((tr("Processing"), summary));
    }
    facts.push((tr("Latency"), latency));
    let mut body = col().key("detail-body").gap(20.0).child(
        text(entry.final_text.clone())
            .key("final")
            .font(p.k.ty.body)
            .color(p.text())
            .wrap(),
    );
    if entry.raw_text.trim() != entry.final_text.trim() {
        body = body.child(
            col()
                .key("raw")
                .gap(6.0)
                .child(section_label(p, "label", tr("Raw transcript")))
                .child(
                    text(entry.raw_text.clone())
                        .key("value")
                        .font(p.k.ty.body)
                        .color(p.muted())
                        .wrap(),
                ),
        );
    }
    body = body.child(ld::facts(p, facts));
    col()
        .key("history-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, tr(entry.kind.label())),
            vec![ld::meta_text(p, &event_age(entry.timestamp_ms))],
            vec![
                copy_action(ui, p, id, false),
                ld::ghost_icon(
                    p,
                    "history-delete",
                    ld::TRASH_ICON,
                    tr("Delete"),
                    move |ui: &mut Ui, _| delete(ui, id),
                ),
            ],
        ))
        .child(ld::card_scroll("detail-scroll", body))
}
