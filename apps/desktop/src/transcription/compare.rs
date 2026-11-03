//! Compare engines: the same audio through several engines, side by side.
//!
//! A developer tool. `wordink compare <wav> --engines groq:whisper-large-v3-turbo,
//! local:parakeet-v3` runs one clip through each engine in turn (in turn, so
//! one engine's CPU use never slows another's timing), prints the texts
//! with a word diff and each engine's latency, real-time factor, request
//! size and cost, and appends a row per engine to `compare.jsonl` in the
//! logs folder. `wordink compare --report` turns that file into a ranking.
//!
//! The logic here is engine-agnostic: an [`Engine`] is anything that turns
//! samples into text, so the tests run it on fakes and make no live call.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::cloud::{self, Provider};

/// The file the rows are appended to, in the logs folder.
pub const LOG_FILE: &str = "compare.jsonl";
const SAMPLE_RATE: f64 = 16_000.0;

/// Which engine: a provider id (`groq`, `openrouter`, `openai`, `deepgram`,
/// `gateway`) or `local`, and a model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineSpec {
    pub provider: String,
    pub model: String,
}

impl EngineSpec {
    /// `provider:model`, or a bare provider for its default model.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (provider, model) = match text.split_once(':') {
            Some((provider, model)) => (provider.trim(), model.trim()),
            None => (text, ""),
        };
        let provider = provider.to_ascii_lowercase();
        if provider != "local" && Provider::from_id(&provider).is_none() {
            return Err(format!(
                "unknown engine {text:?}: use local, groq, openrouter, openai, deepgram or gateway, as provider:model"
            ));
        }
        let model = if model.is_empty() {
            match Provider::from_id(&provider) {
                Some(provider) => provider.default_model().to_string(),
                None => String::new(),
            }
        } else {
            model.to_string()
        };
        Ok(Self { provider, model })
    }

    /// `provider:model`, as the log and the report name it.
    pub fn label(&self) -> String {
        if self.model.is_empty() {
            self.provider.clone()
        } else {
            format!("{}:{}", self.provider, self.model)
        }
    }

    pub fn is_local(&self) -> bool {
        self.provider == "local"
    }

    /// The engines of a comma-separated `--engines` list.
    pub fn parse_list(list: &str) -> Result<Vec<Self>, String> {
        let specs = list
            .split(',')
            .filter(|part| !part.trim().is_empty())
            .map(Self::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if specs.is_empty() {
            return Err("--engines names no engine".into());
        }
        Ok(specs)
    }
}

/// What one engine returned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub text: String,
    /// The engine's own latency; the wall clock around the call when absent.
    pub latency_ms: Option<u64>,
    /// What went over the wire; absent for a local engine.
    pub request_bytes: Option<usize>,
    pub encode_ms: Option<u64>,
    pub cost_usd: Option<f64>,
    /// Where a local engine ran, such as `Apple M1 Max (vulkan)`.
    pub device: Option<String>,
}

/// Something that turns a clip into text.
pub trait Engine {
    fn spec(&self) -> &EngineSpec;
    fn transcribe(&mut self, samples: &[f32]) -> Result<Outcome, String>;
}

/// One engine's row in a comparison, and in `compare.jsonl`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Row {
    pub ts_ms: u64,
    /// The clip, by the start of its SHA-256, so runs on one clip group.
    pub clip: String,
    pub audio_seconds: f64,
    pub engine: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub latency_ms: u64,
    /// Latency over audio length: below 1 is faster than real time.
    pub rtf: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub words: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Word error rate against the reference given when this ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wer: Option<f64>,
    pub text: String,
}

/// A WAV file as 16 kHz mono samples (padded to the shortest clip an engine
/// takes).
pub fn read_wav(path: &Path) -> color_eyre::Result<Vec<f32>> {
    use color_eyre::eyre::WrapErr;
    let mut reader = hound::WavReader::open(path)
        .wrap_err_with(|| format!("could not open {}", path.display()))?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int if spec.bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|sample| sample.map(|sample| sample as f32 / i16::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|sample| sample.map(|sample| sample as f32 / i32::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    let channels = usize::from(spec.channels);
    let mono = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect::<Vec<_>>();
    let mut samples = crate::dictation::resample_for_parakeet(&mono, spec.sample_rate);
    crate::dictation::pad_for_parakeet(&mut samples);
    Ok(samples)
}

/// A short id for a clip.
pub fn clip_id(samples: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for sample in samples {
        hasher.update(sample.to_le_bytes());
    }
    hasher.finalize()[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// Runs `samples` through every engine in order and returns one row each,
/// in the same order. A failing engine gets a row with its error; it never
/// stops the others. With a `reference` transcript each row carries its WER.
pub fn run(engines: &mut [Box<dyn Engine>], samples: &[f32], reference: Option<&str>) -> Vec<Row> {
    let audio_seconds = samples.len() as f64 / SAMPLE_RATE;
    let clip = clip_id(samples);
    engines
        .iter_mut()
        .map(|engine| {
            let started = Instant::now();
            let result = engine.transcribe(samples);
            let wall_ms = started.elapsed().as_millis() as u64;
            let mut row = Row {
                ts_ms: now_ms(),
                clip: clip.clone(),
                audio_seconds,
                engine: engine.spec().label(),
                ok: false,
                error: None,
                latency_ms: wall_ms,
                rtf: 0.0,
                request_bytes: None,
                encode_ms: None,
                cost_usd: None,
                words: 0,
                device: None,
                wer: None,
                text: String::new(),
            };
            match result {
                Ok(outcome) => {
                    row.ok = true;
                    row.latency_ms = outcome.latency_ms.unwrap_or(wall_ms);
                    row.request_bytes = outcome.request_bytes;
                    row.encode_ms = outcome.encode_ms;
                    row.cost_usd = outcome.cost_usd;
                    row.device = outcome.device;
                    row.words = outcome.text.split_whitespace().count();
                    row.wer = reference.map(|reference| wer(reference, &outcome.text));
                    row.text = outcome.text;
                }
                Err(error) => row.error = Some(error),
            }
            row.rtf = if audio_seconds > 0.0 {
                row.latency_ms as f64 / 1000.0 / audio_seconds
            } else {
                0.0
            };
            row
        })
        .collect()
}

// ---- words, diff, WER ------------------------------------------------------

/// The words of `text` for comparison: lowercase, without punctuation (an
/// apostrophe inside a word stays), so "Hello, world." and "hello world"
/// are the same words.
pub fn normalize_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '\u{2019}')
                .flat_map(char::to_lowercase)
                .collect::<String>()
                .trim_matches(|c| c == '\'' || c == '\u{2019}')
                .to_string()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// One step of an alignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffOp {
    Same(String),
    /// The reference word was heard as another.
    Changed(String, String),
    /// A reference word the other text lacks.
    Missing(String),
    /// A word the other text adds.
    Extra(String),
}

/// The fewest-edits alignment of `reference` to `other` and the number of
/// edits (substitutions, insertions, deletions) it takes.
pub fn align(reference: &[String], other: &[String]) -> (Vec<DiffOp>, usize) {
    let (n, m) = (reference.len(), other.len());
    let width = m + 1;
    let mut cost = vec![0_u32; (n + 1) * width];
    for i in 0..=n {
        cost[i * width] = i as u32;
    }
    for (j, slot) in cost.iter_mut().take(m + 1).enumerate() {
        *slot = j as u32;
    }
    for i in 1..=n {
        for j in 1..=m {
            let substitution =
                cost[(i - 1) * width + j - 1] + u32::from(reference[i - 1] != other[j - 1]);
            let deletion = cost[(i - 1) * width + j] + 1;
            let insertion = cost[i * width + j - 1] + 1;
            cost[i * width + j] = substitution.min(deletion).min(insertion);
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let here = cost[i * width + j];
        if i > 0
            && j > 0
            && here == cost[(i - 1) * width + j - 1] + u32::from(reference[i - 1] != other[j - 1])
        {
            ops.push(if reference[i - 1] == other[j - 1] {
                DiffOp::Same(other[j - 1].clone())
            } else {
                DiffOp::Changed(reference[i - 1].clone(), other[j - 1].clone())
            });
            i -= 1;
            j -= 1;
        } else if i > 0 && here == cost[(i - 1) * width + j] + 1 {
            ops.push(DiffOp::Missing(reference[i - 1].clone()));
            i -= 1;
        } else {
            ops.push(DiffOp::Extra(other[j - 1].clone()));
            j -= 1;
        }
    }
    ops.reverse();
    (ops, cost[n * width + m] as usize)
}

/// Word error rate of `hypothesis` against `reference`: edits over the
/// reference's word count. An empty reference is 0 when the hypothesis is
/// empty too, else 1.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let reference = normalize_words(reference);
    let hypothesis = normalize_words(hypothesis);
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    align(&reference, &hypothesis).1 as f64 / reference.len() as f64
}

/// A diff as text: unchanged words as they are, a missing word as `[-word-]`,
/// an added one as `{+word+}`, a changed one as `[-was-]{+now+}`.
pub fn render_diff(ops: &[DiffOp]) -> String {
    ops.iter()
        .map(|op| match op {
            DiffOp::Same(word) => word.clone(),
            DiffOp::Changed(was, now) => format!("[-{was}-]{{+{now}+}}"),
            DiffOp::Missing(word) => format!("[-{word}-]"),
            DiffOp::Extra(word) => format!("{{+{word}+}}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The diff of `text` against `baseline`, as text.
pub fn diff_text(baseline: &str, text: &str) -> String {
    render_diff(&align(&normalize_words(baseline), &normalize_words(text)).0)
}

/// `columns` (a heading and a text each) wrapped to `width` characters and set
/// side by side.
pub fn side_by_side(columns: &[(String, String)], width: usize) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let each = (width.saturating_sub(3 * (columns.len() - 1)) / columns.len()).max(12);
    let wrap = |text: &str| -> Vec<String> {
        let mut lines = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let word: String = word.chars().take(each).collect();
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > each {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    };
    let wrapped: Vec<Vec<String>> = columns
        .iter()
        .map(|(heading, text)| {
            let mut lines = vec![heading.chars().take(each).collect::<String>()];
            lines.push("-".repeat(each.min(heading.chars().count().max(4))));
            lines.extend(wrap(text));
            lines
        })
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = String::new();
    for row in 0..height {
        let cells: Vec<String> = wrapped
            .iter()
            .map(|lines| {
                let cell = lines.get(row).map_or("", String::as_str);
                format!("{cell:<each$}")
            })
            .collect();
        out.push_str(cells.join(" | ").trim_end());
        out.push('\n');
    }
    out
}

// ---- the log and the ranking ----------------------------------------------

/// Where `compare.jsonl` is.
pub fn log_path() -> Option<PathBuf> {
    crate::app_paths::logs_dir()
        .ok()
        .map(|dir| dir.join(LOG_FILE))
}

/// Appends `rows` to `path`, one JSON object a line.
pub fn append(path: &Path, rows: &[Row]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    for row in rows {
        let line = serde_json::to_string(row).map_err(std::io::Error::other)?;
        writeln!(file, "{line}")?;
    }
    Ok(())
}

/// The rows in `path`; a line that is not a row is skipped.
pub fn read(path: &Path) -> std::io::Result<Vec<Row>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// One engine in the ranking.
#[derive(Clone, Debug, PartialEq)]
pub struct Ranked {
    pub rank: usize,
    pub engine: String,
    pub runs: usize,
    pub failures: usize,
    pub median_latency_ms: Option<u64>,
    pub median_rtf: Option<f64>,
    /// Dollars per hour of audio, from the runs that were priced.
    pub cost_per_hour: Option<f64>,
    /// The mean word error rate of the runs that have one.
    pub wer: Option<f64>,
}

fn median<T: Copy + PartialOrd>(mut values: Vec<T>) -> Option<T> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(values[values.len() / 2])
}

/// The engines of `rows` ranked: by word error rate when any run has one
/// (lowest first), then by median latency, and an engine that never
/// succeeded last. `reference` scores every row's text against it (use it on
/// the rows of one clip, with `clip`); a row already carrying its own WER
/// keeps it.
pub fn rank(rows: &[Row], reference: Option<&str>, clip: Option<&str>) -> Vec<Ranked> {
    let mut engines: Vec<String> = Vec::new();
    for row in rows
        .iter()
        .filter(|row| clip.is_none_or(|clip| row.clip == clip))
    {
        if !engines.contains(&row.engine) {
            engines.push(row.engine.clone());
        }
    }
    let mut ranked: Vec<Ranked> = engines
        .into_iter()
        .map(|engine| {
            let mine: Vec<&Row> = rows
                .iter()
                .filter(|row| row.engine == engine && clip.is_none_or(|clip| row.clip == clip))
                .collect();
            let ok: Vec<&&Row> = mine.iter().filter(|row| row.ok).collect();
            let priced: Vec<&&&Row> = ok.iter().filter(|row| row.cost_usd.is_some()).collect();
            let priced_seconds: f64 = priced.iter().map(|row| row.audio_seconds).sum();
            let wers: Vec<f64> = ok
                .iter()
                .filter_map(|row| {
                    row.wer
                        .or_else(|| reference.map(|reference| wer(reference, &row.text)))
                })
                .collect();
            Ranked {
                rank: 0,
                engine,
                runs: mine.len(),
                failures: mine.len() - ok.len(),
                median_latency_ms: median(ok.iter().map(|row| row.latency_ms).collect()),
                median_rtf: median(ok.iter().map(|row| row.rtf).collect()),
                cost_per_hour: (priced_seconds > 0.0).then(|| {
                    priced.iter().filter_map(|row| row.cost_usd).sum::<f64>() / priced_seconds
                        * 3600.0
                }),
                wer: (!wers.is_empty()).then(|| wers.iter().sum::<f64>() / wers.len() as f64),
            }
        })
        .collect();
    let by_wer = ranked.iter().any(|entry| entry.wer.is_some());
    ranked.sort_by(|a, b| {
        let never = a
            .median_latency_ms
            .is_none()
            .cmp(&b.median_latency_ms.is_none());
        let accuracy = if by_wer {
            // An engine without a WER sorts after those that have one.
            a.wer.is_none().cmp(&b.wer.is_none()).then(
                a.wer
                    .partial_cmp(&b.wer)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        } else {
            std::cmp::Ordering::Equal
        };
        never
            .then(accuracy)
            .then(a.median_latency_ms.cmp(&b.median_latency_ms))
            .then(a.engine.cmp(&b.engine))
    });
    for (index, entry) in ranked.iter_mut().enumerate() {
        entry.rank = index + 1;
    }
    ranked
}

/// The ranking as JSON.
pub fn ranking_json(ranked: &[Ranked]) -> String {
    let value: Vec<serde_json::Value> = ranked
        .iter()
        .map(|entry| {
            serde_json::json!({
                "rank": entry.rank,
                "engine": entry.engine,
                "runs": entry.runs,
                "failures": entry.failures,
                "median_latency_ms": entry.median_latency_ms,
                "median_rtf": entry.median_rtf,
                "cost_per_hour_usd": entry.cost_per_hour,
                "wer": entry.wer,
            })
        })
        .collect();
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// The top three engines and, when none of them is local, the best local one:
/// the shortlist to settle on.
pub fn shortlist(ranked: &[Ranked]) -> Vec<&Ranked> {
    let mut list: Vec<&Ranked> = ranked.iter().take(3).collect();
    if !list.iter().any(|entry| entry.engine.starts_with("local:"))
        && let Some(local) = ranked
            .iter()
            .find(|entry| entry.engine.starts_with("local:"))
    {
        list.push(local);
    }
    list
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "-".into(), |value| format!("{:.1}%", value * 100.0))
}

/// The ranking as a text table.
pub fn report_text(ranked: &[Ranked]) -> String {
    if ranked.is_empty() {
        return "No comparison rows yet. Run `wordink compare <wav>` first.\n".into();
    }
    let mut out = format!(
        "{:<4} {:<44} {:>5} {:>5} {:>12} {:>8} {:>11} {:>7}\n",
        "rank", "engine", "runs", "fail", "median ms", "RTF", "$/hour", "WER"
    );
    for entry in ranked {
        out.push_str(&format!(
            "{:<4} {:<44} {:>5} {:>5} {:>12} {:>8} {:>11} {:>7}\n",
            entry.rank,
            entry.engine,
            entry.runs,
            entry.failures,
            entry
                .median_latency_ms
                .map_or_else(|| "-".into(), |ms| ms.to_string()),
            entry
                .median_rtf
                .map_or_else(|| "-".into(), |rtf| format!("{rtf:.3}")),
            entry
                .cost_per_hour
                .map_or_else(|| "-".into(), |cost| format!("{cost:.3}")),
            percent(entry.wer),
        ));
    }
    out.push_str("\nShortlist (top three, plus the best local engine):\n");
    for entry in shortlist(ranked) {
        out.push_str(&format!("  {}. {}\n", entry.rank, entry.engine));
    }
    out
}

/// A comparison as text: the metrics, the texts side by side, and each
/// engine's word diff against `baseline`.
pub fn comparison_text(rows: &[Row], reference: Option<&str>, width: usize) -> String {
    let mut out = format!(
        "{:<44} {:>9} {:>7} {:>11} {:>10} {:>7}\n",
        "engine", "ms", "RTF", "bytes", "cost $", "WER"
    );
    for row in rows {
        if !row.ok {
            out.push_str(&format!(
                "{:<44} failed: {}\n",
                row.engine,
                row.error.as_deref().unwrap_or("unknown error")
            ));
            continue;
        }
        out.push_str(&format!(
            "{:<44} {:>9} {:>7.3} {:>11} {:>10} {:>7}\n",
            row.engine,
            row.latency_ms,
            row.rtf,
            row.request_bytes
                .map_or_else(|| "-".into(), |bytes| bytes.to_string()),
            row.cost_usd
                .map_or_else(|| "-".into(), |cost| format!("{cost:.6}")),
            percent(row.wer),
        ));
    }
    let columns: Vec<(String, String)> = rows
        .iter()
        .filter(|row| row.ok)
        .map(|row| (row.engine.clone(), row.text.clone()))
        .collect();
    out.push_str("\nSide by side\n");
    out.push_str(&side_by_side(&columns, width));
    // The baseline is the reference, else the first engine that answered.
    let first = rows.iter().find(|row| row.ok);
    let baseline = reference
        .map(|reference| ("the reference".to_string(), reference.to_string()))
        .or_else(|| first.map(|row| (row.engine.clone(), row.text.clone())));
    if let Some((name, baseline)) = baseline {
        out.push_str(&format!(
            "\nWord diff against {name} ([-missing-] {{+added+}}):\n"
        ));
        for row in rows.iter().filter(|row| row.ok) {
            if reference.is_none() && first.is_some_and(|first| std::ptr::eq(first, row)) {
                continue;
            }
            out.push_str(&format!(
                "  {}: {}\n",
                row.engine,
                diff_text(&baseline, &row.text)
            ));
        }
    }
    out
}

// ---- real engines ----------------------------------------------------------

struct CloudEngine {
    spec: EngineSpec,
    provider: Provider,
    language: Option<String>,
}

impl Engine for CloudEngine {
    fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<Outcome, String> {
        let sent = cloud::transcribe_unkept(
            self.provider,
            &self.spec.model,
            cloud::gateway_url().as_deref(),
            samples,
            self.language.as_deref(),
        )?;
        Ok(Outcome {
            text: sent.text,
            latency_ms: Some(sent.total_ms as u64),
            request_bytes: Some(sent.stats.request_bytes),
            encode_ms: Some(sent.stats.encode_ms as u64),
            cost_usd: sent.cost,
            device: None,
        })
    }
}

#[cfg(not(target_os = "macos"))]
struct LocalEngine {
    spec: EngineSpec,
    transcriber: crate::transcription::local_transcriber::LocalTranscriber,
}

#[cfg(not(target_os = "macos"))]
impl Engine for LocalEngine {
    fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    fn transcribe(&mut self, samples: &[f32]) -> Result<Outcome, String> {
        // The model is loaded and warm: this times a dictation, not a launch.
        let started = Instant::now();
        let text = self
            .transcriber
            .transcribe(samples)
            .map_err(|error| format!("{error:#}"))?;
        Ok(Outcome {
            text,
            latency_ms: Some(started.elapsed().as_millis() as u64),
            request_bytes: None,
            encode_ms: None,
            cost_usd: Some(0.0),
            device: Some(self.transcriber.device_label().to_string()),
        })
    }
}

/// The engine for `spec`. A cloud engine needs a key; a local one loads its
/// model now (so its load is not in the timing).
pub fn build_engine(spec: &EngineSpec, language: Option<&str>) -> Result<Box<dyn Engine>, String> {
    if spec.is_local() {
        return build_local(spec, language);
    }
    let provider = Provider::from_id(&spec.provider)
        .ok_or_else(|| format!("unknown engine {}", spec.label()))?;
    Ok(Box::new(CloudEngine {
        spec: spec.clone(),
        provider,
        language: cloud::language_hint(language.unwrap_or("auto")),
    }))
}

#[cfg(not(target_os = "macos"))]
fn build_local(spec: &EngineSpec, language: Option<&str>) -> Result<Box<dyn Engine>, String> {
    let settings = crate::app_settings::AppSettings::read().unwrap_or_default();
    let model = if spec.model.is_empty() {
        settings.transcription.model.as_str().to_owned()
    } else {
        spec.model.clone()
    };
    let selection = crate::control::model_selection(&settings, &model, language)
        .map_err(|error| error.message)?;
    let transcriber = crate::transcription::local_transcriber::LocalTranscriber::load(&selection)
        .map_err(|error| format!("{error:#}"))?;
    Ok(Box::new(LocalEngine {
        spec: EngineSpec {
            provider: "local".into(),
            model,
        },
        transcriber,
    }))
}

#[cfg(target_os = "macos")]
fn build_local(_: &EngineSpec, _: Option<&str>) -> Result<Box<dyn Engine>, String> {
    Err("local engines run inside the app on macOS; compare cloud engines here".into())
}

/// The engines Compare uses when none are named: every cloud provider with a
/// key at its default model (Groq also at `whisper-large-v3`), and the
/// selected local model when it is installed. Resolves keys, so it may wait
/// on the keyring: not for the UI thread.
pub fn default_specs() -> Vec<EngineSpec> {
    let mut specs = Vec::new();
    for provider in Provider::ALL {
        if !cloud::has_key_blocking(provider) {
            continue;
        }
        specs.push(EngineSpec {
            provider: provider.id().into(),
            model: provider.default_model().into(),
        });
        if provider == Provider::Groq {
            specs.push(EngineSpec {
                provider: "groq".into(),
                model: "whisper-large-v3".into(),
            });
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let settings = crate::app_settings::AppSettings::read().unwrap_or_default();
        let model = settings.transcription.model.as_str().to_owned();
        if let Ok(selection) = crate::control::model_selection(&settings, &model, None)
            && crate::transcription::models::is_installed(
                crate::transcription::models::definition(selection.model),
                &selection.language,
            )
        {
            specs.push(EngineSpec {
                provider: "local".into(),
                model,
            });
        }
    }
    specs
}

/// Builds each of `specs`; an engine that cannot be built (no key, a model
/// that will not load) becomes a failed row instead of stopping the rest.
pub fn run_specs(
    specs: &[EngineSpec],
    samples: &[f32],
    reference: Option<&str>,
    language: Option<&str>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for spec in specs {
        match build_engine(spec, language) {
            Ok(engine) => rows.extend(run(&mut [engine], samples, reference)),
            Err(error) => {
                let mut failed = run(
                    &mut [Box::new(Unbuildable {
                        spec: spec.clone(),
                        error,
                    })],
                    samples,
                    reference,
                );
                rows.append(&mut failed);
            }
        }
    }
    rows
}

struct Unbuildable {
    spec: EngineSpec,
    error: String,
}

impl Engine for Unbuildable {
    fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    fn transcribe(&mut self, _: &[f32]) -> Result<Outcome, String> {
        Err(self.error.clone())
    }
}

#[cfg(test)]
mod tests;
