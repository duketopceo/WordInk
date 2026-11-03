//! `wordink usage`: the Usage pane's numbers in the terminal, read straight
//! from History and the cloud request log so the app need not be running.

use std::io::{self, Write};

use clap::{Args, ValueEnum};
use color_eyre::eyre::{Result, WrapErr};
use serde::Serialize;

use crate::history::HistoryEntry;
use crate::history::usage::{
    self, EngineReport, UsageSummary, Window, cost_label, duration_label, latency_line,
};
use crate::transcription::cloud::request_log::{self, Origin, RequestRecord};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum WindowArg {
    Today,
    #[default]
    Week,
    Month,
    All,
}

impl WindowArg {
    const fn window(self) -> Window {
        match self {
            Self::Today => Window::Today,
            Self::Week => Window::Week,
            Self::Month => Window::Month,
            Self::All => Window::All,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Week => "week",
            Self::Month => "month",
            Self::All => "all",
        }
    }
}

#[derive(Args, Debug)]
pub struct UsageArgs {
    /// How far back to add up.
    #[arg(long, value_enum, default_value_t)]
    pub window: WindowArg,
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
struct Totals {
    sessions: u64,
    words: u64,
    audio_seconds: f64,
    cost_usd: f64,
}

#[derive(Serialize)]
pub struct UsageReport {
    version: u32,
    window: &'static str,
    totals: Totals,
    /// Dictations: p50 and p95 transcription time, real-time factor, error
    /// rate and cost per audio hour, per engine and model.
    dictation_engines: Vec<EngineReport>,
    /// The same from the cloud request log for meeting chunks.
    meeting_engines: Vec<EngineReport>,
}

/// Adds up `entries` and `requests` over `window` as of `now`.
pub fn report(
    entries: &[HistoryEntry],
    requests: &[RequestRecord],
    window: WindowArg,
    now: u64,
) -> UsageReport {
    let since = window.window().since_ms(now, usage::local_day_start(now));
    let summary: UsageSummary = usage::summarize(entries, since);
    let stats = usage::dictation_stats(
        entries,
        requests,
        since,
        crate::history::stats::local_day(now),
        crate::history::stats::local_day,
        usage::local_hour,
    );
    UsageReport {
        version: 1,
        window: window.name(),
        totals: Totals {
            sessions: summary.sessions,
            words: summary.words,
            audio_seconds: summary.audio_seconds,
            cost_usd: summary.cost_usd,
        },
        dictation_engines: stats.latency.iter().map(EngineReport::from).collect(),
        meeting_engines: usage::request_latency(requests, Some(Origin::Meeting), since)
            .iter()
            .map(EngineReport::from)
            .collect(),
    }
}

fn engine_text(row: &EngineReport) -> String {
    latency_line(&row.into())
}

pub fn text(report: &UsageReport) -> String {
    let totals = &report.totals;
    let mut text = format!(
        "Usage ({}): {} sessions, {} words, {} audio, {}\n",
        report.window,
        totals.sessions,
        totals.words,
        duration_label(totals.audio_seconds),
        cost_label(totals.cost_usd)
    );
    let mut section = |title: &str, rows: &[EngineReport]| {
        if !rows.is_empty() {
            text.push_str(&format!("{title}:\n"));
            for row in rows {
                text.push_str(&format!("  {}\n", engine_text(row)));
            }
        }
    };
    section("Dictation engines", &report.dictation_engines);
    section("Meeting engines", &report.meeting_engines);
    text
}

/// `wordink usage`.
pub fn run(args: &UsageArgs) -> Result<()> {
    let root = crate::app_paths::support_dir()?;
    let entries = crate::history::read_snapshot(&root.join("history.json"))
        .wrap_err("could not read history.json")?;
    let requests = request_log::read(&root.join(request_log::FILE));
    let report = report(
        &entries,
        &requests,
        args.window,
        crate::desktop::events::now_ms(),
    );
    let mut out = io::stdout().lock();
    if args.json {
        serde_json::to_writer_pretty(&mut out, &report)?;
        writeln!(out)?;
    } else {
        write!(out, "{}", text(&report))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryKind, UsageRecord};

    const NOW: u64 = 1_793_000_000_000;

    fn entry(id: u64, inference_ms: u64, cost_usd: f64) -> HistoryEntry {
        HistoryEntry {
            id,
            timestamp_ms: NOW - 60_000 * id,
            kind: HistoryKind::Dictation,
            raw_text: "one two".into(),
            final_text: "one two".into(),
            application: None,
            processing: None,
            audio_ms: 5_000,
            inference_ms,
            total_ms: inference_ms,
            usage: Some(UsageRecord {
                provider: "openrouter".into(),
                model: "microsoft/mai-transcribe-2".into(),
                audio_seconds: 5.0,
                words: 2,
                cost_usd,
            }),
        }
    }

    fn request(origin: Origin, ok: bool) -> RequestRecord {
        RequestRecord {
            timestamp_ms: NOW - 1_000,
            origin,
            provider: "openrouter".into(),
            endpoint_kind: "transcription".into(),
            model: "microsoft/mai-transcribe-2".into(),
            audio_ms: 5_000,
            latency_ms: 4_000,
            error_class: (!ok).then(|| "rate_limited".to_owned()),
            ..RequestRecord::default()
        }
    }

    #[test]
    fn the_report_has_totals_and_per_engine_p50_p95_error_rate_and_cost() {
        let entries = [entry(1, 4_000, 0.000139), entry(2, 6_000, 0.000139)];
        let requests = [
            request(Origin::Dictation, true),
            request(Origin::Dictation, true),
            request(Origin::Dictation, false),
            request(Origin::Meeting, true),
        ];
        let report = report(&entries, &requests, WindowArg::Week, NOW);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["window"], "week");
        assert_eq!(json["totals"]["sessions"], 2);
        assert_eq!(json["totals"]["words"], 4);
        let engine = &json["dictation_engines"][0];
        assert_eq!(engine["provider"], "openrouter");
        assert_eq!(engine["model"], "microsoft/mai-transcribe-2");
        assert_eq!(engine["samples"], 2);
        assert_eq!((engine["p50_ms"].as_u64(), engine["p95_ms"].as_u64()), (Some(5_000), Some(5_900)));
        assert_eq!(engine["requests"], 3);
        assert_eq!(engine["failed"], 1);
        assert!((engine["error_rate"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert!((engine["real_time_factor"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!(
            (engine["cost_per_audio_hour_usd"].as_f64().unwrap() - 0.1).abs() < 1e-3,
            "{engine}"
        );
        let meeting = &json["meeting_engines"][0];
        assert_eq!((meeting["requests"].as_u64(), meeting["failed"].as_u64()), (Some(1), Some(0)));
    }

    #[test]
    fn the_text_names_each_engine_and_an_empty_window_is_just_the_totals() {
        let entries = [entry(1, 4_000, 0.000139)];
        let report = report(&entries, &[request(Origin::Dictation, true)], WindowArg::All, NOW);
        let text = text(&report);
        assert!(text.starts_with("Usage (all): 1 sessions, 2 words"), "{text}");
        assert!(text.contains("Dictation engines:\n  openrouter \u{b7} microsoft/mai-transcribe-2: p50 4.0 s"), "{text}");
        assert!(!text.contains("Meeting engines"), "{text}");
        let empty = super::text(&super::report(&[], &[], WindowArg::Today, NOW));
        assert_eq!(empty, "Usage (today): 0 sessions, 0 words, 0s audio, $0.0000\n");
    }

    #[test]
    fn the_usage_command_parses_a_window_and_json() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            usage: UsageArgs,
        }
        let cli = Cli::try_parse_from(["usage", "--window", "month", "--json"]).unwrap();
        assert_eq!((cli.usage.window, cli.usage.json), (WindowArg::Month, true));
        let default = Cli::try_parse_from(["usage"]).unwrap();
        assert_eq!((default.usage.window, default.usage.json), (WindowArg::Week, false));
        assert!(Cli::try_parse_from(["usage", "--window", "year"]).is_err());
    }
}
