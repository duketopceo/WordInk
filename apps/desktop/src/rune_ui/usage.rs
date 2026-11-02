//! The Usage pane (R25): words dictated, sessions, audio, estimated cost per
//! provider and model, and time saved, for today, 7 days, 30 days and all
//! time. It adds up the usage records History keeps (KTD5), so it covers
//! what retention keeps and nothing more.

use std::time::{Duration, Instant};

use rune::{El, Tx, col, text};
use rune_kit::{Select, SelectAppearance};

use super::controls::{CardRow, Paint, described_row, page, section};
use super::list_detail as ld;
use super::{Menu, Ui};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::history::History;
use crate::history::usage::{TYPING_WPM, UsageSummary, Window, cost_label, duration_label};

/// How often the open pane adds the records up again.
const REFRESH: Duration = Duration::from_secs(2);

pub(super) struct UsagePane {
    pub(super) window: Window,
    pub(super) summary: UsageSummary,
    loaded: Option<Instant>,
}

impl Default for UsagePane {
    fn default() -> Self {
        Self {
            window: Window::Week,
            summary: UsageSummary::default(),
            loaded: None,
        }
    }
}

impl UsagePane {
    pub(super) fn reload(&mut self, store: Option<&History>) {
        self.loaded = Some(Instant::now());
        if let Some(store) = store {
            self.summary = store.usage(self.window);
        }
    }

    /// While the pane shows, adds the records up again every couple of
    /// seconds, so a dictation made meanwhile appears.
    pub(super) fn poll(&mut self, store: Option<&History>, visible: bool) -> bool {
        if !visible || store.is_none() {
            return false;
        }
        if self.loaded.is_some_and(|at| at.elapsed() < REFRESH) {
            return false;
        }
        let before = self.summary.clone();
        self.reload(store);
        self.summary != before
    }

    /// A week of mixed local, Groq, OpenAI and OpenRouter dictation.
    pub(super) fn fixture() -> Self {
        use crate::history::usage::ModelUsage;
        let row =
            |provider: &str, model: &str, sessions, words, audio_seconds, cost_usd| ModelUsage {
                provider: provider.into(),
                model: model.into(),
                sessions,
                words,
                audio_seconds,
                cost_usd,
            };
        let by_model = vec![
            row("openai", "gpt-4o-mini-transcribe", 12, 640, 410.0, 0.0205),
            row("groq", "whisper-large-v3-turbo", 96, 5_120, 2_980.0, 0.0331),
            row(
                "openrouter",
                "google/gemini-2.5-flash-lite",
                8,
                410,
                260.0,
                0.0042,
            ),
            row("local", "parakeet_v3", 41, 1_870, 1_020.0, 0.0),
        ];
        Self {
            window: Window::Week,
            summary: UsageSummary {
                sessions: by_model.iter().map(|r| r.sessions).sum(),
                words: by_model.iter().map(|r| r.words).sum(),
                audio_seconds: by_model.iter().map(|r| r.audio_seconds).sum(),
                cost_usd: by_model.iter().map(|r| r.cost_usd).sum(),
                by_model,
            },
            loaded: Some(Instant::now()),
        }
    }

    pub(super) fn fixture_empty() -> Self {
        Self {
            loaded: Some(Instant::now()),
            ..Self::default()
        }
    }
}

fn set_window(ui: &mut Ui, window: Window) {
    ui.open = None;
    ui.usage.window = window;
    let store = ui.history.store().cloned();
    ui.usage.reload(store.as_ref());
}

fn value(p: Paint, text_value: String) -> El {
    text(text_value)
        .key("value")
        .font(p.k.ty.ui_medium)
        .color(p.text())
}

/// The window choice in the header, and the page.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    let labels: Vec<&str> = Window::ALL.iter().map(|w| tr(w.label())).collect();
    let current = tr(ui.usage.window.label());
    let picker = Select::new("usage-window", &labels)
        .appearance(SelectAppearance::Inline)
        .current(Some(current))
        .open(ui.open == Some(Menu::UsageWindow))
        .on_toggle(|ui: &mut Ui| {
            ui.open = (ui.open != Some(Menu::UsageWindow)).then_some(Menu::UsageWindow);
        })
        .on_close(|ui: &mut Ui| ui.open = None)
        .on_choose(|ui: &mut Ui, choice: &str, _: &mut Tx| {
            if let Some(window) = Window::ALL.into_iter().find(|w| tr(w.label()) == choice) {
                set_window(ui, window);
            }
        })
        .build(p.k, layer);

    let summary = &ui.usage.summary;
    if summary.sessions == 0 {
        let body = ld::empty_state(
            p,
            "usage-empty",
            ld::HISTORY_ICON,
            tr("No usage in this period."),
            Some(tr(
                "Dictations are counted here while History keeps them. Turn History on to start.",
            )),
        );
        return (
            vec![picker],
            col().key("usage-pane").grow(1.0).shrink().child(body),
        );
    }

    let saved = summary.time_saved_seconds();
    let totals: Vec<CardRow> = vec![
        described_row(
            p,
            "usage-words",
            tr("Words dictated"),
            None,
            value(p, summary.words.to_string()),
        )
        .into(),
        described_row(
            p,
            "usage-sessions",
            tr("Sessions"),
            None,
            value(p, summary.sessions.to_string()),
        )
        .into(),
        described_row(
            p,
            "usage-audio",
            tr("Audio recorded"),
            None,
            value(p, duration_label(summary.audio_seconds)),
        )
        .into(),
        described_row(
            p,
            "usage-cost",
            tr("Estimated cost"),
            Some((
                tr("From public prices; local transcription is free").to_owned(),
                p.muted(),
            )),
            value(p, cost_label(summary.cost_usd)),
        )
        .into(),
        described_row(
            p,
            "usage-saved",
            tr("Time saved"),
            Some((
                tr_named(
                    "Words ÷ {wpm} wpm typing speed − time spent speaking",
                    &[("wpm", &format!("{TYPING_WPM:.0}"))],
                ),
                p.muted(),
            )),
            value(p, duration_label(saved)),
        )
        .into(),
    ];

    let rows: Vec<CardRow> = summary
        .by_model
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let provider = crate::transcription::cloud::Provider::from_id(&row.provider)
                .map_or_else(|| tr("Local model").to_owned(), |p| p.name().to_owned());
            let detail = tr_named(
                "{sessions} sessions · {words} words · {audio} audio",
                &[
                    ("sessions", &row.sessions.to_string()),
                    ("words", &row.words.to_string()),
                    ("audio", &duration_label(row.audio_seconds)),
                ],
            );
            described_row(
                p,
                &format!("usage-model{index}"),
                &format!("{provider} · {}", row.model),
                Some((detail, p.muted())),
                value(p, cost_label(row.cost_usd)),
            )
            .into()
        })
        .collect();

    let content = page("content")
        .child(section(
            p,
            "usage-totals",
            &tr_fill("Totals · {}", current),
            totals,
        ))
        .child(section(
            p,
            "usage-models",
            tr("By provider and model"),
            rows,
        ))
        .child(
            text(tr(
                "Costs are estimates; your provider's bill is the record.",
            ))
            .key("usage-note")
            .font(super::controls::DESCRIPTION)
            .color(p.muted())
            .wrap()
            .mx(super::controls::METRICS.row_px),
        );
    (vec![picker], super::scroll_body("scroll", content))
}
