//! The Usage pane's numbers (R25, KTD5): what the retained usage records add
//! up to over a window, per provider and model. Nothing is stored beyond
//! History itself, so the totals cover what retention keeps.

use super::HistoryEntry;

/// The typing speed time saved is measured against, in words per minute.
pub const TYPING_WPM: f64 = 40.0;

/// How far back a summary reaches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Window {
    Today,
    Week,
    Month,
    All,
}

impl Window {
    pub const ALL: [Self; 4] = [Self::Today, Self::Week, Self::Month, Self::All];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::All => "All time",
        }
    }

    /// The earliest timestamp inside the window. Today starts at local
    /// midnight; the others are rolling 7 and 30 days.
    pub fn since_ms(self, now_ms: u64, today_start_ms: u64) -> u64 {
        const DAY: u64 = 24 * 60 * 60 * 1000;
        match self {
            Self::Today => today_start_ms,
            Self::Week => now_ms.saturating_sub(7 * DAY),
            Self::Month => now_ms.saturating_sub(30 * DAY),
            Self::All => 0,
        }
    }
}

/// One provider and model's share of a window.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub sessions: u64,
    pub words: u64,
    pub audio_seconds: f64,
    /// Estimated, in US dollars.
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageSummary {
    pub sessions: u64,
    pub words: u64,
    pub audio_seconds: f64,
    pub cost_usd: f64,
    /// Costliest first, then by provider and model.
    pub by_model: Vec<ModelUsage>,
}

impl UsageSummary {
    /// Words ÷ 40 wpm, less the minutes spent speaking, in seconds. Negative
    /// when speaking took longer than typing would have.
    pub fn time_saved_seconds(&self) -> f64 {
        self.words as f64 / TYPING_WPM * 60.0 - self.audio_seconds
    }
}

/// Adds up the usage records of `entries` made at or after `since_ms`.
/// Entries saved before usage was recorded have no record and are not
/// counted.
pub fn summarize<'a>(
    entries: impl IntoIterator<Item = &'a HistoryEntry>,
    since_ms: u64,
) -> UsageSummary {
    let mut summary = UsageSummary::default();
    for entry in entries {
        let Some(usage) = entry
            .usage
            .as_ref()
            .filter(|_| entry.timestamp_ms >= since_ms)
        else {
            continue;
        };
        let words = u64::from(usage.words);
        summary.sessions += 1;
        summary.words += words;
        summary.audio_seconds += usage.audio_seconds;
        summary.cost_usd += usage.cost_usd;
        let row = match summary
            .by_model
            .iter_mut()
            .find(|row| row.provider == usage.provider && row.model == usage.model)
        {
            Some(row) => row,
            None => {
                summary.by_model.push(ModelUsage {
                    provider: usage.provider.clone(),
                    model: usage.model.clone(),
                    sessions: 0,
                    words: 0,
                    audio_seconds: 0.0,
                    cost_usd: 0.0,
                });
                summary.by_model.last_mut().expect("just pushed")
            }
        };
        row.sessions += 1;
        row.words += words;
        row.audio_seconds += usage.audio_seconds;
        row.cost_usd += usage.cost_usd;
    }
    summary.by_model.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.model.cmp(&b.model))
    });
    summary
}

/// Local midnight at or before `now_ms`.
pub fn local_day_start(now_ms: u64) -> u64 {
    let iso = crate::meeting::query::iso_local(now_ms);
    let field = |range: std::ops::Range<usize>| iso.get(range)?.parse::<u64>().ok();
    match (field(11..13), field(14..16), field(17..19)) {
        (Some(h), Some(m), Some(s)) => {
            now_ms.saturating_sub(((h * 60 + m) * 60 + s) * 1000 + now_ms % 1000)
        }
        _ => now_ms - now_ms % 86_400_000,
    }
}

/// `1h 05m`, `7m`, or `45s`: a duration for the pane.
pub fn duration_label(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    match (total / 3600, (total % 3600) / 60) {
        (0, 0) => format!("{total}s"),
        (0, minutes) => format!("{minutes}m"),
        (hours, minutes) => format!("{hours}h {minutes:02}m"),
    }
}

/// `$0.0123`: costs here are fractions of a cent, so four places under a
/// dollar, two above.
pub fn cost_label(usd: f64) -> String {
    if usd >= 1.0 {
        format!("${usd:.2}")
    } else {
        format!("${usd:.4}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryEntry, HistoryKind, UsageRecord};

    const DAY: u64 = 24 * 60 * 60 * 1000;
    // 2026-11-02 12:00 UTC-ish; midnight of that day is NOW - 12 h.
    const NOW: u64 = 1_793_000_000_000;
    const MIDNIGHT: u64 = NOW - 12 * 60 * 60 * 1000;

    fn entry(id: u64, age_ms: u64, usage: Option<(&str, &str, f64, u32, f64)>) -> HistoryEntry {
        HistoryEntry {
            id,
            timestamp_ms: NOW - age_ms,
            kind: HistoryKind::Dictation,
            raw_text: String::new(),
            final_text: String::new(),
            application: None,
            processing: None,
            audio_ms: 0,
            inference_ms: 0,
            total_ms: 0,
            usage: usage.map(
                |(provider, model, audio_seconds, words, cost_usd)| UsageRecord {
                    provider: provider.into(),
                    model: model.into(),
                    audio_seconds,
                    words,
                    cost_usd,
                },
            ),
        }
    }

    fn fixture() -> Vec<HistoryEntry> {
        vec![
            // Today.
            entry(
                1,
                60_000,
                Some(("groq", "whisper-large-v3-turbo", 30.0, 80, 0.0003)),
            ),
            entry(
                2,
                3_600_000,
                Some(("groq", "whisper-large-v3-turbo", 60.0, 150, 0.0007)),
            ),
            entry(
                3,
                2 * 3_600_000,
                Some(("local", "parakeet_v3", 20.0, 40, 0.0)),
            ),
            // Three days ago, and 20 days ago, and a year ago.
            entry(4, 3 * DAY, Some(("openai", "whisper-1", 120.0, 300, 0.012))),
            entry(5, 20 * DAY, Some(("deepgram", "nova-3", 60.0, 100, 0.0043))),
            entry(
                6,
                365 * DAY,
                Some(("groq", "whisper-large-v3-turbo", 10.0, 20, 0.0001)),
            ),
            // Saved before usage was recorded.
            entry(7, 1_000, None),
        ]
    }

    fn windowed(window: Window) -> UsageSummary {
        summarize(&fixture(), window.since_ms(NOW, MIDNIGHT))
    }

    #[test]
    fn today_adds_up_words_sessions_audio_and_cost_per_provider_and_model() {
        let today = windowed(Window::Today);
        assert_eq!(
            today.sessions, 3,
            "the entry with no usage record is not a session"
        );
        assert_eq!(today.words, 270);
        assert!((today.audio_seconds - 110.0).abs() < 1e-9);
        assert!((today.cost_usd - 0.001).abs() < 1e-12);
        let groq = &today.by_model[0];
        assert_eq!(
            (groq.provider.as_str(), groq.model.as_str()),
            ("groq", "whisper-large-v3-turbo")
        );
        assert_eq!((groq.sessions, groq.words), (2, 230));
        assert!((groq.audio_seconds - 90.0).abs() < 1e-9);
        assert!(
            (groq.cost_usd - 0.001).abs() < 1e-12,
            "the cost total per provider and model"
        );
        let local = &today.by_model[1];
        assert_eq!((local.provider.as_str(), local.cost_usd), ("local", 0.0));
        assert_eq!(today.by_model.len(), 2);
    }

    #[test]
    fn the_windows_reach_back_today_seven_thirty_days_and_all_time() {
        let sessions = |window| windowed(window).sessions;
        assert_eq!(sessions(Window::Today), 3);
        assert_eq!(sessions(Window::Week), 4);
        assert_eq!(sessions(Window::Month), 5);
        assert_eq!(sessions(Window::All), 6);
        let all = windowed(Window::All);
        assert_eq!(all.words, 690);
        assert!((all.cost_usd - (0.0003 + 0.0007 + 0.012 + 0.0043 + 0.0001)).abs() < 1e-12);
        // Costliest model first.
        assert_eq!(all.by_model[0].model, "whisper-1");
        let week = windowed(Window::Week);
        assert!(
            week.by_model
                .iter()
                .any(|row| row.provider == "openai" && (row.cost_usd - 0.012).abs() < 1e-12)
        );
        assert!(!week.by_model.iter().any(|row| row.provider == "deepgram"));
    }

    #[test]
    fn time_saved_is_words_over_forty_wpm_less_the_time_spent_speaking() {
        let today = windowed(Window::Today);
        // 270 words at 40 wpm is 6.75 min = 405 s; 110 s were spoken.
        assert!((today.time_saved_seconds() - 295.0).abs() < 1e-9);
        let slow = UsageSummary {
            words: 10,
            audio_seconds: 60.0,
            ..UsageSummary::default()
        };
        assert!(
            slow.time_saved_seconds() < 0.0,
            "speaking slowly saves nothing; the sign is kept"
        );
        assert_eq!(UsageSummary::default().time_saved_seconds(), 0.0);
        assert_eq!(TYPING_WPM, 40.0);
    }

    #[test]
    fn an_empty_window_is_an_empty_summary() {
        let none = summarize(&Vec::<HistoryEntry>::new(), 0);
        assert_eq!(none, UsageSummary::default());
        let future = summarize(&fixture(), NOW + DAY);
        assert_eq!(future.sessions, 0);
    }

    #[test]
    fn durations_and_costs_read_for_a_person() {
        assert_eq!(duration_label(0.0), "0s");
        assert_eq!(duration_label(45.4), "45s");
        assert_eq!(duration_label(295.0), "4m");
        assert_eq!(duration_label(3900.0), "1h 05m");
        assert_eq!(duration_label(-5.0), "0s");
        assert_eq!(cost_label(0.0), "$0.0000");
        assert_eq!(cost_label(0.00215), "$0.0022");
        assert_eq!(cost_label(12.5), "$12.50");
    }

    #[test]
    fn local_midnight_is_at_or_before_now_and_less_than_a_day_back() {
        let start = local_day_start(NOW);
        assert!(start <= NOW && NOW - start < DAY + 3_600_000);
    }
}
