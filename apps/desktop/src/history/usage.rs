//! The Usage pane's numbers (R25, KTD5): what the retained usage records add
//! up to over a window, per provider and model. Nothing is stored beyond
//! History itself, so the totals cover what retention keeps.

use super::HistoryEntry;
use super::stats::{self, HistoryStats};
use crate::transcription::cloud::request_log::{Origin as RequestOrigin, RequestRecord};

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

/// The `percent`th percentile of `sorted` (ascending), by linear
/// interpolation between the two nearest ranks, rounded to a millisecond.
/// A single sample is every percentile of itself; no samples, no answer.
/// With an even count the median is the mean of the two middle samples.
pub fn percentile(sorted: &[u64], percent: f64) -> Option<u64> {
    let last = sorted.len().checked_sub(1)?;
    let rank = (percent.clamp(0.0, 100.0) / 100.0) * last as f64;
    let (below, above) = (rank.floor() as usize, rank.ceil() as usize);
    let low = sorted[below] as f64;
    let high = sorted[above] as f64;
    Some((low + (high - low) * (rank - below as f64)).round() as u64)
}

/// Transcription time over audio time: below 1 is faster than real time.
/// `None` without audio.
pub fn real_time_factor(latency_ms: u64, audio_ms: u64) -> Option<f64> {
    (audio_ms > 0).then(|| latency_ms as f64 / audio_ms as f64)
}

/// How one engine did over a window: how long it took (percentiles over
/// the dictations that recorded a time), how that compares with the audio,
/// how often requests to it failed, and what an hour of audio cost.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelLatency {
    pub provider: String,
    pub model: String,
    /// Dictations that recorded a transcription time.
    pub dictations: u64,
    /// Mean transcription time in milliseconds, rounded.
    pub average_ms: u64,
    /// Median transcription time; `None` when no dictation recorded a time.
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    /// Total transcription time over total audio time of those dictations.
    pub real_time_factor: Option<f64>,
    /// Cloud requests logged in the window, failed ones included. Zero for a
    /// local model, which makes no requests.
    pub requests: u64,
    pub failed: u64,
    /// US dollars paid per hour of audio: what the provider charged for the
    /// dictations (its returned cost, else the bundled price table over
    /// the audio). `None` when nothing was charged.
    pub cost_per_hour: Option<f64>,
}

impl ModelLatency {
    /// The share of logged requests that failed, 0 to 1.
    pub fn error_rate(&self) -> Option<f64> {
        (self.requests > 0).then(|| self.failed as f64 / self.requests as f64)
    }
}

/// One engine's numbers as the CLI prints them as JSON.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct EngineReport {
    pub provider: String,
    pub model: String,
    /// Requests or dictations that recorded a time.
    pub samples: u64,
    pub average_ms: u64,
    pub p50_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub real_time_factor: Option<f64>,
    pub requests: u64,
    pub failed: u64,
    pub error_rate: Option<f64>,
    pub cost_per_audio_hour_usd: Option<f64>,
}

impl From<&ModelLatency> for EngineReport {
    fn from(row: &ModelLatency) -> Self {
        Self {
            provider: row.provider.clone(),
            model: row.model.clone(),
            samples: row.dictations,
            average_ms: row.average_ms,
            p50_ms: row.p50_ms,
            p95_ms: row.p95_ms,
            real_time_factor: row.real_time_factor,
            requests: row.requests,
            failed: row.failed,
            error_rate: row.error_rate(),
            cost_per_audio_hour_usd: row.cost_per_hour,
        }
    }
}

impl From<&EngineReport> for ModelLatency {
    fn from(row: &EngineReport) -> Self {
        Self {
            provider: row.provider.clone(),
            model: row.model.clone(),
            dictations: row.samples,
            average_ms: row.average_ms,
            p50_ms: row.p50_ms,
            p95_ms: row.p95_ms,
            real_time_factor: row.real_time_factor,
            requests: row.requests,
            failed: row.failed,
            cost_per_hour: row.cost_per_audio_hour_usd,
        }
    }
}

/// One engine's line for the CLI and diagnostics: `openrouter ·
/// microsoft/mai-transcribe-2: p50 4.1 s, p95 6.8 s, 0.95× real time, 11%
/// errors (1 of 9 requests), $0.100 per audio hour, 8 samples`. English: the
/// terminal and a bug report are not translated.
pub fn latency_line(row: &ModelLatency) -> String {
    let mut parts = Vec::new();
    match (row.p50_ms, row.p95_ms) {
        (Some(p50), Some(p95)) => {
            parts.push(format!("p50 {}", latency_label(p50)));
            parts.push(format!("p95 {}", latency_label(p95)));
        }
        _ => parts.push("no timings".to_owned()),
    }
    if let Some(factor) = row.real_time_factor {
        parts.push(format!("{} real time", factor_label(factor)));
    }
    if let Some(rate) = row.error_rate() {
        parts.push(format!(
            "{} errors ({} of {} requests)",
            percent_label(rate),
            row.failed,
            row.requests
        ));
    }
    if let Some(cost) = row.cost_per_hour {
        parts.push(format!("${cost:.3} per audio hour"));
    }
    if row.dictations > 0 {
        parts.push(format!("{} samples", row.dictations));
    }
    format!("{} \u{b7} {}: {}", row.provider, row.model, parts.join(", "))
}

/// The personal numbers of one window: the dictations themselves, not just
/// their usage records.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DictationStats {
    /// Dictations, words, speed, streak, active days, top apps, and the
    /// share Output changed.
    pub history: HistoryStats,
    /// Mean words per dictation, rounded; zero without dictations.
    pub average_words: u32,
    pub longest_words: u64,
    /// Local hour of day (0 to 23) with the most dictations; the earliest
    /// wins a tie.
    pub busiest_hour: Option<u32>,
    /// Fastest median first, then by provider and model.
    pub latency: Vec<ModelLatency>,
}

impl DictationStats {
    /// Speaking speed over the typing speed time saved is measured against.
    pub fn speed_vs_typing(&self) -> Option<f64> {
        self.history
            .words_per_minute
            .map(|wpm| f64::from(wpm) / TYPING_WPM)
    }
}

/// Sums up the dictations of `entries` made at or after `since_ms`. `today`,
/// `day_of` and `hour_of` turn timestamps into local days and hours (the
/// app's own clock, or a fixed one in tests).
///
/// `requests` is the cloud request log; the dictation requests made at or
/// after `since_ms` give each engine its error rate.
pub fn dictation_stats<'a>(
    entries: impl IntoIterator<Item = &'a HistoryEntry>,
    requests: &[RequestRecord],
    since_ms: u64,
    today: i64,
    day_of: impl Fn(u64) -> i64,
    hour_of: impl Fn(u64) -> u32,
) -> DictationStats {
    let entries: Vec<&HistoryEntry> = entries
        .into_iter()
        .filter(|entry| entry.timestamp_ms >= since_ms)
        .collect();
    let history = stats::summarize(entries.iter().copied(), today, day_of);
    let mut stats = DictationStats::default();
    let mut hours = [0_u64; 24];
    let mut latency: Vec<Engine> = Vec::new();
    for entry in &entries {
        let words = entry.final_text.split_whitespace().count() as u64;
        stats.longest_words = stats.longest_words.max(words);
        hours[(hour_of(entry.timestamp_ms) as usize).min(23)] += 1;
        if let Some(usage) = &entry.usage {
            let row = engine_row(&mut latency, &usage.provider, &usage.model);
            if entry.inference_ms > 0 {
                row.times.push(entry.inference_ms);
                row.audio_ms += (usage.audio_seconds * 1000.0).round() as u64;
            }
            row.audio_seconds += usage.audio_seconds;
            row.cost_usd += usage.cost_usd;
        }
    }
    if history.dictations > 0 {
        stats.average_words = (history.words as f64 / history.dictations as f64).round() as u32;
    }
    let most = hours.iter().copied().max().unwrap_or(0);
    stats.busiest_hour = (most > 0)
        .then(|| hours.iter().position(|count| *count == most))
        .flatten()
        .map(|hour| hour as u32);
    for request in requests.iter().filter(|request| {
        request.timestamp_ms >= since_ms && request.origin == RequestOrigin::Dictation
    }) {
        let row = engine_row(&mut latency, &request.provider, &request.model);
        row.requests += 1;
        row.failed += u64::from(!request.ok());
    }
    stats.latency = latency
        .into_iter()
        // A row appears for an engine that timed a dictation or failed a
        // request; one that only has a usage record (no time) does not.
        .filter(|row| !row.times.is_empty() || row.failed > 0)
        .map(Engine::finish)
        .collect();
    sort_latency(&mut stats.latency);
    stats.history = history;
    stats
}

/// The engines of the cloud request log over a window, from the requests
/// alone: a successful request's wall time is its latency, its audio and
/// cost are summed, and a failed one counts against the error rate. `origin`
/// narrows to dictation or meeting requests. A request that raised an error
/// has no usable time and is left out of the percentiles.
pub fn request_latency(
    requests: &[RequestRecord],
    origin: Option<RequestOrigin>,
    since_ms: u64,
) -> Vec<ModelLatency> {
    let mut rows: Vec<Engine> = Vec::new();
    for request in requests.iter().filter(|request| {
        request.timestamp_ms >= since_ms && origin.is_none_or(|origin| request.origin == origin)
    }) {
        let row = engine_row(&mut rows, &request.provider, &request.model);
        row.requests += 1;
        if request.ok() {
            row.times.push(request.latency_ms);
            row.audio_ms += request.audio_ms;
            row.audio_seconds += request.audio_ms as f64 / 1000.0;
            row.cost_usd += request.cost_usd.unwrap_or(0.0);
        } else {
            row.failed += 1;
        }
    }
    let mut latency: Vec<ModelLatency> = rows.into_iter().map(Engine::finish).collect();
    sort_latency(&mut latency);
    latency
}

fn sort_latency(rows: &mut [ModelLatency]) {
    rows.sort_by(|a, b| {
        a.p50_ms
            .unwrap_or(u64::MAX)
            .cmp(&b.p50_ms.unwrap_or(u64::MAX))
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.model.cmp(&b.model))
    });
}

impl Engine {
    fn finish(mut self) -> ModelLatency {
        self.times.sort_unstable();
        let total: u64 = self.times.iter().sum();
        let count = self.times.len() as u64;
        ModelLatency {
            provider: self.provider,
            model: self.model,
            dictations: count,
            average_ms: if count > 0 {
                (total as f64 / count as f64).round() as u64
            } else {
                0
            },
            p50_ms: percentile(&self.times, 50.0),
            p95_ms: percentile(&self.times, 95.0),
            real_time_factor: real_time_factor(total, self.audio_ms),
            requests: self.requests,
            failed: self.failed,
            cost_per_hour: (self.cost_usd > 0.0 && self.audio_seconds > 0.0)
                .then(|| self.cost_usd / self.audio_seconds * 3600.0),
        }
    }
}

/// One engine's samples while a window is added up.
struct Engine {
    provider: String,
    model: String,
    times: Vec<u64>,
    /// Audio of the dictations that recorded a time.
    audio_ms: u64,
    /// Audio and cost of every dictation with a usage record.
    audio_seconds: f64,
    cost_usd: f64,
    requests: u64,
    failed: u64,
}

fn engine_row<'a>(rows: &'a mut Vec<Engine>, provider: &str, model: &str) -> &'a mut Engine {
    let index = rows
        .iter()
        .position(|row| row.provider == provider && row.model == model)
        .unwrap_or_else(|| {
            rows.push(Engine {
                provider: provider.to_owned(),
                model: model.to_owned(),
                times: Vec::new(),
                audio_ms: 0,
                audio_seconds: 0.0,
                cost_usd: 0.0,
                requests: 0,
                failed: 0,
            });
            rows.len() - 1
        });
    &mut rows[index]
}

/// The local hour of day (0 to 23) of a timestamp.
pub fn local_hour(timestamp_ms: u64) -> u32 {
    let iso = crate::meeting::query::iso_local(timestamp_ms);
    iso.get(11..13)
        .and_then(|hour| hour.parse::<u32>().ok())
        .filter(|hour| *hour < 24)
        .unwrap_or((timestamp_ms / 3_600_000 % 24) as u32)
}

/// `9 am`, `12 pm`: one hour on the clock.
fn hour_label(hour: u32) -> String {
    let hour = hour % 24;
    let suffix = if hour < 12 { "am" } else { "pm" };
    format!(
        "{} {suffix}",
        if hour.is_multiple_of(12) {
            12
        } else {
            hour % 12
        }
    )
}

/// `9–10 am`, or `11 am–12 pm` across noon or midnight: the hour that
/// starts at `hour`.
pub fn hour_range_label(hour: u32) -> String {
    let (start, end) = (hour_label(hour), hour_label(hour + 1));
    match (start.split_once(' '), end.split_once(' ')) {
        (Some((from, a)), Some((to, b))) if a == b => format!("{from}–{to} {a}"),
        _ => format!("{start}–{end}"),
    }
}

/// `0.45×`: a real-time factor, two places.
pub fn factor_label(factor: f64) -> String {
    format!("{factor:.2}\u{d7}")
}

/// `2%`, or `0.4%` under one percent: a share of requests that failed.
pub fn percent_label(share: f64) -> String {
    let percent = share * 100.0;
    if percent > 0.0 && percent < 1.0 {
        format!("{percent:.1}%")
    } else {
        format!("{percent:.0}%")
    }
}

/// `850 ms`, or `1.2 s` from a second up.
pub fn latency_label(milliseconds: u64) -> String {
    if milliseconds < 1_000 {
        format!("{milliseconds} ms")
    } else {
        format!("{:.1} s", milliseconds as f64 / 1_000.0)
    }
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

    fn dictation(
        id: u64,
        hour: u64,
        text: &str,
        app: Option<&str>,
        engine: Option<(&str, &str)>,
        audio_ms: u64,
        inference_ms: u64,
    ) -> HistoryEntry {
        let mut entry = entry(id, 0, None);
        // Fixed UTC time: day 20_000 at `hour`.
        entry.timestamp_ms = 20_000 * DAY + hour * 3_600_000 + 5_000;
        entry.raw_text = text.into();
        entry.final_text = text.into();
        entry.application = app.map(str::to_owned);
        entry.audio_ms = audio_ms;
        entry.inference_ms = inference_ms;
        entry.usage = engine.map(|(provider, model)| UsageRecord {
            provider: provider.into(),
            model: model.into(),
            audio_seconds: audio_ms as f64 / 1000.0,
            words: 0,
            cost_usd: 0.0,
        });
        entry
    }

    fn utc_day(ms: u64) -> i64 {
        (ms / DAY) as i64
    }

    fn utc_hour(ms: u64) -> u32 {
        (ms / 3_600_000 % 24) as u32
    }

    fn habits() -> Vec<HistoryEntry> {
        let groq = Some(("groq", "whisper-large-v3-turbo"));
        let local = Some(("local", "parakeet_v3"));
        vec![
            // 9 am, twice; 14 pm once.
            dictation(1, 9, "one two three four", Some("Zed"), groq, 6_000, 400),
            dictation(2, 9, "five six", Some("Zed"), groq, 3_000, 600),
            dictation(
                3,
                14,
                "seven eight nine ten eleven twelve",
                Some("Slack"),
                local,
                12_000,
                1_500,
            ),
            // Before usage was recorded: no engine, no latency.
            dictation(4, 14, "hi", None, None, 0, 0),
        ]
    }

    fn habit_stats(since_ms: u64) -> DictationStats {
        dictation_stats(&habits(), &[], since_ms, 20_000, utc_day, utc_hour)
    }

    #[test]
    fn speed_and_the_typing_comparison_come_from_the_entries_with_audio() {
        let stats = habit_stats(0);
        // 12 words in 21 s of audio (the entry without audio is skipped).
        assert_eq!(stats.history.words_per_minute, Some(34));
        let ratio = stats.speed_vs_typing().unwrap();
        assert!((ratio - 34.0 / 40.0).abs() < 1e-9);
        assert_eq!(DictationStats::default().speed_vs_typing(), None);
    }

    #[test]
    fn streak_and_active_days_cover_the_window() {
        let mut entries = habits();
        entries.push(dictation(5, 8, "x", None, None, 0, 0));
        entries.last_mut().unwrap().timestamp_ms -= DAY;
        entries.push(dictation(6, 8, "y", None, None, 0, 0));
        entries.last_mut().unwrap().timestamp_ms -= 5 * DAY;
        let stats = dictation_stats(&entries, &[], 0, 20_000, utc_day, utc_hour);
        assert_eq!(
            (stats.history.streak_days, stats.history.active_days),
            (2, 3)
        );
        // A window that starts today drops the older days.
        let today = dictation_stats(&entries, &[], 20_000 * DAY, 20_000, utc_day, utc_hour);
        assert_eq!(
            (today.history.streak_days, today.history.active_days),
            (1, 1)
        );
    }

    #[test]
    fn average_and_longest_dictation_are_in_words() {
        let stats = habit_stats(0);
        // 4 + 2 + 6 + 1 words over 4 dictations is 3.25.
        assert_eq!(stats.average_words, 3);
        assert_eq!(stats.longest_words, 6);
        let empty = habit_stats(u64::MAX);
        assert_eq!(empty, DictationStats::default());
    }

    #[test]
    fn top_apps_and_the_changed_share_come_through() {
        let mut entries = habits();
        entries[0].final_text = "One two three four.".into();
        let stats = dictation_stats(&entries, &[], 0, 20_000, utc_day, utc_hour);
        let apps: Vec<_> = stats
            .history
            .top_apps
            .iter()
            .map(|app| (app.application.as_str(), app.dictations))
            .collect();
        assert_eq!(apps, [("Zed", 2), ("Slack", 1)]);
        assert_eq!(stats.history.changed_percent(), Some(25));
    }

    #[test]
    fn the_busiest_hour_is_the_most_dictated_and_ties_go_to_the_earliest() {
        assert_eq!(habit_stats(0).busiest_hour, Some(9));
        let mut entries = habits();
        entries.push(dictation(5, 14, "z", None, None, 0, 0));
        // Two at 9, three at 14.
        let stats = dictation_stats(&entries, &[], 0, 20_000, utc_day, utc_hour);
        assert_eq!(stats.busiest_hour, Some(14));
        assert_eq!(DictationStats::default().busiest_hour, None);
        // One at 9 and one at 14 is a tie.
        let even = dictation_stats(&habits()[1..3], &[], 0, 20_000, utc_day, utc_hour);
        assert_eq!(even.busiest_hour, Some(9));
    }

    #[test]
    fn latency_is_the_mean_transcription_time_per_engine_fastest_first() {
        let latency = habit_stats(0).latency;
        let rows: Vec<_> = latency
            .iter()
            .map(|row| (row.provider.as_str(), row.dictations, row.average_ms))
            .collect();
        // Groq: (400 + 600) / 2. Local: 1500. The entry with no engine or
        // time is left out.
        assert_eq!(rows, [("groq", 2, 500), ("local", 1, 1_500)]);
    }

    #[test]
    fn the_neural_engine_and_its_gpu_fallback_are_separate_rows_with_their_own_times() {
        let local = |id, model: &str, audio_ms, inference_ms| {
            dictation(
                id,
                9,
                "x",
                None,
                Some(("local", model)),
                audio_ms,
                inference_ms,
            )
        };
        // Four served by the Neural Engine, one that fell back to the GPU.
        let entries = vec![
            local(1, "parakeet_v3_ane", 10_000, 600),
            local(2, "parakeet_v3_ane", 10_000, 620),
            local(3, "parakeet_v3_ane", 10_000, 580),
            local(4, "parakeet_v3_ane", 10_000, 900),
            local(5, "parakeet_v3", 10_000, 840),
        ];
        let latency = dictation_stats(&entries, &[], 0, 20_000, utc_day, utc_hour).latency;
        let row = |model: &str| latency.iter().find(|row| row.model == model).unwrap();
        let ane = row("parakeet_v3_ane");
        let gpu = row("parakeet_v3");
        assert_eq!((ane.dictations, gpu.dictations), (4, 1));
        assert_eq!(ane.p50_ms, Some(610));
        // Percentiles interpolate: 620 + 0.85 * (900 - 620).
        assert_eq!(ane.p95_ms, Some(858));
        assert_eq!((gpu.p50_ms, gpu.p95_ms), (Some(840), Some(840)));
        // Real-time factor is each row's own time over its own audio.
        assert!((ane.real_time_factor.unwrap() - 2_700.0 / 40_000.0).abs() < 1e-9);
        assert!((gpu.real_time_factor.unwrap() - 840.0 / 10_000.0).abs() < 1e-9);
    }

    #[test]
    fn hours_and_latencies_read_for_a_person() {
        assert_eq!(hour_range_label(9), "9–10 am");
        assert_eq!(hour_range_label(11), "11 am–12 pm");
        assert_eq!(hour_range_label(12), "12–1 pm");
        assert_eq!(hour_range_label(23), "11 pm–12 am");
        assert_eq!(hour_range_label(0), "12–1 am");
        assert_eq!(latency_label(850), "850 ms");
        assert_eq!(latency_label(1_240), "1.2 s");
        assert!(local_hour(NOW) < 24);
    }

    #[test]
    fn percentiles_interpolate_and_handle_small_samples() {
        assert_eq!(percentile(&[], 50.0), None, "no samples, no answer");
        assert_eq!(percentile(&[700], 50.0), Some(700));
        assert_eq!(percentile(&[700], 95.0), Some(700), "one sample is every percentile");
        // Odd count: the middle sample.
        assert_eq!(percentile(&[100, 300, 200_000], 50.0), Some(300));
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 50.0), Some(3));
        // Even count: the mean of the two middle samples.
        assert_eq!(percentile(&[100, 200], 50.0), Some(150));
        assert_eq!(percentile(&[100, 200, 300, 400], 50.0), Some(250));
        // p95 reaches toward the slowest, never past it.
        assert_eq!(percentile(&[100, 200], 95.0), Some(195));
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 95.0), Some(5));
        let twenty: Vec<u64> = (1..=20).map(|n| n * 100).collect();
        assert_eq!(percentile(&twenty, 95.0), Some(1_905));
        assert_eq!(percentile(&twenty, 100.0), Some(2_000));
        assert_eq!(percentile(&twenty, 0.0), Some(100));
        assert_eq!(percentile(&twenty, 250.0), Some(2_000), "clamped");
    }

    #[test]
    fn the_real_time_factor_is_transcription_time_over_audio_time() {
        assert_eq!(real_time_factor(4_400, 60_000).map(|f| (f * 1000.0).round()), Some(73.0));
        assert_eq!(real_time_factor(3_000, 3_000), Some(1.0));
        assert_eq!(real_time_factor(500, 0), None, "no audio, no factor");
        assert_eq!(real_time_factor(0, 1_000), Some(0.0));
        assert_eq!(factor_label(0.0733), "0.07\u{d7}");
        assert_eq!(factor_label(1.0), "1.00\u{d7}");
    }

    fn request(
        age_ms: u64,
        provider: &str,
        model: &str,
        ok: bool,
        latency_ms: u64,
        audio_ms: u64,
        cost: Option<f64>,
    ) -> RequestRecord {
        RequestRecord {
            timestamp_ms: 20_000 * DAY + 12 * 3_600_000 - age_ms,
            provider: provider.into(),
            endpoint_kind: "transcription".into(),
            model: model.into(),
            audio_ms,
            latency_ms,
            cost_usd: cost,
            error_class: (!ok).then(|| "unavailable".to_owned()),
            ..RequestRecord::default()
        }
    }

    #[test]
    fn latency_rows_give_p50_p95_real_time_error_rate_and_cost_per_hour() {
        let mai = Some(("openrouter", "microsoft/mai-transcribe-2"));
        let mut entries = vec![
            dictation(1, 9, "a b", None, mai, 5_000, 4_000),
            dictation(2, 9, "c d", None, mai, 5_000, 6_000),
            dictation(3, 9, "e f", None, mai, 10_000, 5_000),
        ];
        // What the provider charged: $0.10 an hour of audio.
        for entry in &mut entries {
            let usage = entry.usage.as_mut().unwrap();
            usage.cost_usd = usage.audio_seconds * 0.10 / 3600.0;
        }
        let requests = [
            request(1_000, "openrouter", "microsoft/mai-transcribe-2", true, 4_000, 5_000, Some(0.0001)),
            request(2_000, "openrouter", "microsoft/mai-transcribe-2", true, 6_000, 5_000, Some(0.0001)),
            request(3_000, "openrouter", "microsoft/mai-transcribe-2", true, 5_000, 10_000, Some(0.0003)),
            request(4_000, "openrouter", "microsoft/mai-transcribe-2", false, 900, 5_000, None),
            // A meeting chunk does not count against dictation.
            RequestRecord {
                origin: crate::transcription::cloud::request_log::Origin::Meeting,
                ..request(5_000, "openrouter", "microsoft/mai-transcribe-2", false, 1, 1, None)
            },
        ];
        let stats = dictation_stats(&entries, &requests, 0, 20_000, utc_day, utc_hour);
        let row = &stats.latency[0];
        assert_eq!((row.provider.as_str(), row.model.as_str()), ("openrouter", "microsoft/mai-transcribe-2"));
        assert_eq!(row.dictations, 3);
        assert_eq!((row.p50_ms, row.p95_ms), (Some(5_000), Some(5_900)));
        assert_eq!(row.average_ms, 5_000);
        // 15 s of transcription over 20 s of audio.
        assert!((row.real_time_factor.unwrap() - 0.75).abs() < 1e-9);
        assert_eq!((row.requests, row.failed), (4, 1));
        assert!((row.error_rate().unwrap() - 0.25).abs() < 1e-9);
        assert!((row.cost_per_hour.unwrap() - 0.10).abs() < 1e-9);
        // Local models make no requests: no error rate, and nothing paid.
        let local = dictation_stats(&habits(), &[], 0, 20_000, utc_day, utc_hour);
        let parakeet = local.latency.iter().find(|r| r.provider == "local").unwrap();
        assert_eq!((parakeet.error_rate(), parakeet.cost_per_hour), (None, None));
    }

    #[test]
    fn engines_sort_by_median_and_one_that_only_failed_still_shows() {
        let fast = Some(("groq", "whisper-large-v3-turbo"));
        let slow = Some(("openrouter", "microsoft/mai-transcribe-2"));
        let entries = vec![
            dictation(1, 9, "a", None, slow, 5_000, 4_500),
            dictation(2, 9, "b", None, fast, 5_000, 400),
        ];
        let requests = [request(1, "openai", "whisper-1", false, 100, 1_000, None)];
        let stats = dictation_stats(&entries, &requests, 0, 20_000, utc_day, utc_hour);
        let order: Vec<_> = stats.latency.iter().map(|r| r.provider.as_str()).collect();
        assert_eq!(order, ["groq", "openrouter", "openai"], "no timing sorts last");
        let failed = &stats.latency[2];
        assert_eq!((failed.p50_ms, failed.dictations, failed.failed), (None, 0, 1));
        assert_eq!(failed.error_rate(), Some(1.0));
        // Requests before the window do not count.
        let old = dictation_stats(&entries, &requests, 20_000 * DAY + 13 * 3_600_000, 20_000, utc_day, utc_hour);
        assert!(old.latency.is_empty());
        // Nothing at all: nothing to show.
        assert!(dictation_stats(&Vec::<HistoryEntry>::new(), &[], 0, 20_000, utc_day, utc_hour).latency.is_empty());
    }

    #[test]
    fn the_request_log_alone_gives_meeting_engine_numbers() {
        use crate::transcription::cloud::request_log::Origin;
        let meeting = |latency, ok| RequestRecord {
            origin: Origin::Meeting,
            ..request(1, "openrouter", "microsoft/mai-transcribe-2", ok, latency, 8_000, Some(0.0002))
        };
        let requests = [meeting(3_000, true), meeting(5_000, true), meeting(99, false)];
        let rows = request_latency(&requests, Some(Origin::Meeting), 0);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!((row.p50_ms, row.requests, row.failed), (Some(4_000), 3, 1));
        assert!((row.real_time_factor.unwrap() - 0.5).abs() < 1e-9);
        assert!((row.cost_per_hour.unwrap() - 0.0004 / 16.0 * 3600.0).abs() < 1e-9);
        assert!(request_latency(&requests, Some(Origin::Dictation), 0).is_empty());
        assert_eq!(request_latency(&requests, None, 0).len(), 1);
        let line = latency_line(row);
        assert!(line.starts_with("openrouter \u{b7} microsoft/mai-transcribe-2: p50 4.0 s, p95 4.9 s"), "{line}");
        assert!(line.contains("0.50\u{d7} real time") && line.contains("33% errors (1 of 3 requests)"), "{line}");
        assert!(line.contains("per audio hour") && line.contains("2 samples"), "{line}");
    }

    #[test]
    fn rates_and_factors_read_for_a_person() {
        assert_eq!(percent_label(0.0), "0%");
        assert_eq!(percent_label(0.004), "0.4%");
        assert_eq!(percent_label(0.02), "2%");
        assert_eq!(percent_label(1.0), "100%");
    }
}
