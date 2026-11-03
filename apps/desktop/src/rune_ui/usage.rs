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
use crate::history::stats::{AppCount, HistoryStats};
use crate::history::usage::{
    DictationStats, ModelLatency, TYPING_WPM, UsageSummary, Window, cost_label, duration_label,
    hour_range_label, latency_label,
};

/// How often the open pane adds the records up again.
const REFRESH: Duration = Duration::from_secs(2);

pub(super) struct UsagePane {
    pub(super) window: Window,
    pub(super) summary: UsageSummary,
    pub(super) stats: DictationStats,
    loaded: Option<Instant>,
}

impl Default for UsagePane {
    fn default() -> Self {
        Self {
            window: Window::Week,
            summary: UsageSummary::default(),
            stats: DictationStats::default(),
            loaded: None,
        }
    }
}

impl UsagePane {
    pub(super) fn reload(&mut self, store: Option<&History>) {
        self.loaded = Some(Instant::now());
        if let Some(store) = store {
            self.summary = store.usage(self.window);
            self.stats = store.dictation_stats(self.window);
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
        let before = (self.summary.clone(), self.stats.clone());
        self.reload(store);
        (&self.summary, &self.stats) != (&before.0, &before.1)
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
            stats: fixture_stats(),
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

/// A week of dictation habits for the preview: mornings, mostly in a few
/// apps, with a fast cloud engine and a slower local one.
fn fixture_stats() -> DictationStats {
    let app = |application: &str, dictations| AppCount {
        application: application.into(),
        dictations,
    };
    let latency = |provider: &str, model: &str, dictations, average_ms| ModelLatency {
        provider: provider.into(),
        model: model.into(),
        dictations,
        average_ms,
    };
    DictationStats {
        history: HistoryStats {
            dictations: 157,
            words: 8_040,
            audio_ms: 4_670_000,
            words_per_minute: Some(103),
            changed_by_output: 38,
            streak_days: 6,
            active_days: 7,
            top_apps: vec![
                app("Zed", 61),
                app("Slack", 44),
                app("Mail", 27),
                app("Notes", 16),
                app("Safari", 9),
            ],
        },
        average_words: 51,
        longest_words: 412,
        busiest_hour: Some(9),
        latency: vec![
            latency("groq", "whisper-large-v3-turbo", 96, 380),
            latency("openai", "gpt-4o-mini-transcribe", 12, 940),
            latency("local", "parakeet_v3", 41, 1_450),
        ],
    }
}

/// `1 day` or `3 days`.
fn counted(count: u64, one: &str, other: &str) -> String {
    tr_fill(if count == 1 { one } else { other }, &count.to_string())
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
                if ui.settings.history_retention == crate::history::HistoryRetention::Off {
                    "Dictations are counted here while History keeps them. Turn History on to start."
                } else {
                    "No dictations in the selected period."
                },
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

    let mut habits: Vec<CardRow> = Vec::new();
    let stats = &ui.usage.stats;
    if let (Some(wpm), Some(ratio)) = (stats.history.words_per_minute, stats.speed_vs_typing()) {
        habits.push(
            described_row(
                p,
                "usage-speed",
                tr("Speaking speed"),
                Some((
                    tr_named(
                        "{times}× typing speed ({wpm} wpm)",
                        &[
                            ("times", &format!("{ratio:.1}")),
                            ("wpm", &format!("{TYPING_WPM:.0}")),
                        ],
                    ),
                    p.muted(),
                )),
                value(p, tr_fill("{} wpm", &wpm.to_string())),
            )
            .into(),
        );
    }
    if stats.history.dictations > 0 {
        let history = &stats.history;
        let mut simple = |key: &str, label: &str, shown: String| {
            habits.push(described_row(p, key, tr(label), None, value(p, shown)).into());
        };
        simple(
            "usage-streak",
            "Daily streak",
            counted(u64::from(history.streak_days), "{} day", "{} days"),
        );
        simple(
            "usage-active-days",
            "Active days",
            counted(u64::from(history.active_days), "{} day", "{} days"),
        );
        simple(
            "usage-average-length",
            "Average dictation",
            counted(u64::from(stats.average_words), "{} word", "{} words"),
        );
        simple(
            "usage-longest",
            "Longest dictation",
            counted(stats.longest_words, "{} word", "{} words"),
        );
        if let Some(hour) = stats.busiest_hour {
            simple("usage-busiest", "Most active", hour_range_label(hour));
        }
        habits.push(
            described_row(
                p,
                "usage-changed",
                tr("Changed by Output"),
                Some((
                    tr("Replacements, modes or rewriting altered the text").to_owned(),
                    p.muted(),
                )),
                value(
                    p,
                    format!("{}%", history.changed_percent().unwrap_or_default()),
                ),
            )
            .into(),
        );
    }
    let apps: Vec<CardRow> = stats
        .history
        .top_apps
        .iter()
        .enumerate()
        .map(|(index, app)| {
            described_row(
                p,
                &format!("usage-app{index}"),
                &app.application,
                None,
                value(p, app.dictations.to_string()),
            )
            .into()
        })
        .collect();
    let latency: Vec<CardRow> = stats
        .latency
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let provider = crate::transcription::cloud::Provider::from_id(&row.provider)
                .map_or_else(|| tr("Local model").to_owned(), |p| p.name().to_owned());
            described_row(
                p,
                &format!("usage-latency{index}"),
                &format!("{provider} · {}", row.model),
                Some((
                    counted(row.dictations, "{} dictation", "{} dictations"),
                    p.muted(),
                )),
                value(p, latency_label(row.average_ms)),
            )
            .into()
        })
        .collect();

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
        .children((!habits.is_empty()).then(|| {
            section(
                p,
                "usage-habits",
                &tr_fill("Dictation habits · {}", current),
                habits,
            )
        }))
        .children(
            (!apps.is_empty())
                .then(|| section(p, "usage-apps", &tr_fill("Top apps · {}", current), apps)),
        )
        .children((!latency.is_empty()).then(|| {
            section(
                p,
                "usage-latency",
                &tr_fill("Average transcription time · {}", current),
                latency,
            )
        }))
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
