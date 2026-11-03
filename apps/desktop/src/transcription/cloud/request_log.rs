//! The record of every cloud transcription request: which provider, API and
//! model, how long, how big, what came back, what it cost. It feeds the Usage
//! pane's error rates, `wordink usage` and Copy diagnostics, and is also
//! emitted as a `tracing` event so the debug log carries the same fields.
//!
//! A record holds numbers and names only. It never holds a key, an
//! authorization header, audio or transcript text: those never reach this
//! module. It lives beside `history.json`, owner-only, bounded in size, is
//! not written while history is off, and goes when history is cleared.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file beside `history.json`.
pub const FILE: &str = "cloud-requests.jsonl";
/// Records kept after a trim.
const KEEP: usize = 2_000;
/// The file is trimmed to [`KEEP`] records once it grows past this.
const TRIM_AT_BYTES: u64 = 1_000_000;

/// What the request was made for.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Dictation,
    Meeting,
}

impl Origin {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Meeting => "meeting",
        }
    }
}

/// One finished cloud request (after any retry).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RequestRecord {
    pub timestamp_ms: u64,
    pub origin: Origin,
    /// `groq`, `openrouter`, ...: the provider's id.
    pub provider: String,
    /// `chat` or `transcription`: which API of the provider was called.
    pub endpoint_kind: String,
    pub model: String,
    pub audio_ms: u64,
    pub request_bytes: u64,
    /// The last HTTP status, or none when no response came (cannot connect).
    pub status: Option<u16>,
    /// The provider's request id header, when it sends one.
    pub request_id: Option<String>,
    /// OpenRouter's generation id (`X-Generation-Id`).
    pub generation_id: Option<String>,
    /// The provider OpenRouter routed to (`X-Provider-Name`).
    pub routed_provider: Option<String>,
    /// Wall time of the whole request, retries included.
    pub latency_ms: u64,
    /// What the provider charged, else the bundled price estimate.
    pub cost_usd: Option<f64>,
    /// Extra attempts made after the first.
    pub retries: u32,
    /// A short class of the failure (`key_rejected`, `rate_limited`, ...), or
    /// none for a success.
    pub error_class: Option<String>,
}

impl RequestRecord {
    pub fn ok(&self) -> bool {
        self.error_class.is_none()
    }

    /// Sends the record to the log as one structured event (the debug log's
    /// `engine` subsystem bridges `tracing`). Only the fields below exist.
    pub fn emit(&self) {
        macro_rules! event {
            ($level:ident, $message:literal) => {
                tracing::$level!(
                    origin = self.origin.name(),
                    provider = %self.provider,
                    endpoint_kind = %self.endpoint_kind,
                    model = %self.model,
                    audio_ms = self.audio_ms,
                    request_bytes = self.request_bytes,
                    status = self.status,
                    request_id = self.request_id.as_deref(),
                    generation_id = self.generation_id.as_deref(),
                    routed_provider = self.routed_provider.as_deref(),
                    latency_ms = self.latency_ms,
                    cost_usd = self.cost_usd,
                    retries = self.retries,
                    error_class = self.error_class.as_deref(),
                    $message
                )
            };
        }
        if self.ok() {
            event!(info, "cloud transcription request");
        } else {
            event!(warn, "cloud transcription request failed");
        }
    }
}

/// Appends `record` to `path`, trimming the file when it has grown large.
pub fn append(path: &Path, record: &RequestRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    line.push(b'\n');
    let mut options = fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(&line)?;
    if fs::metadata(path).is_ok_and(|meta| meta.len() > TRIM_AT_BYTES) {
        trim(path)?;
    }
    Ok(())
}

fn trim(path: &Path) -> std::io::Result<()> {
    let records = read(path);
    let kept = &records[records.len().saturating_sub(KEEP)..];
    let mut bytes = Vec::new();
    for record in kept {
        bytes.extend(serde_json::to_vec(record).map_err(std::io::Error::other)?);
        bytes.push(b'\n');
    }
    let temporary = path.with_extension("jsonl.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&temporary)?.write_all(&bytes)?;
    fs::rename(temporary, path)
}

/// Every readable record, oldest first. A missing file and a line that does
/// not parse (a torn write) are skipped.
pub fn read(path: &Path) -> Vec<RequestRecord> {
    fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Deletes the log: it goes with the history it describes.
pub fn clear(path: &Path) {
    let _ = fs::remove_file(path);
}

/// The log beside `history.json`, or `None` without a support directory.
pub fn default_path() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|dir| dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(timestamp_ms: u64) -> RequestRecord {
        RequestRecord {
            timestamp_ms,
            provider: "openrouter".into(),
            endpoint_kind: "transcription".into(),
            model: "microsoft/mai-transcribe-2".into(),
            audio_ms: 5_000,
            latency_ms: 4_000,
            ..RequestRecord::default()
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wordink-request-log-{name}-{}-{}",
            std::process::id(),
            crate::history::now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(FILE)
    }

    #[test]
    fn records_round_trip_in_order_and_torn_lines_are_skipped() {
        let path = temp("roundtrip");
        append(&path, &record(1)).unwrap();
        let mut failed = record(2);
        failed.error_class = Some("rate_limited".into());
        failed.status = Some(429);
        append(&path, &failed).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"timestamp_ms\":")
            .unwrap();
        let records = read(&path);
        assert_eq!(records.len(), 2);
        assert!(records[0].ok() && !records[1].ok());
        assert_eq!(records[1].status, Some(429));
        assert!(read(&path.with_file_name("none.jsonl")).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_log_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp("mode");
        append(&path, &record(1)).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_large_log_is_trimmed_to_the_newest_records() {
        let path = temp("trim");
        let mut text = String::new();
        let line = serde_json::to_string(&record(0)).unwrap();
        // Past the trim size with distinguishable timestamps.
        let count = (TRIM_AT_BYTES as usize / line.len()) + 50;
        for n in 0..count {
            let mut r = record(n as u64);
            r.timestamp_ms = n as u64;
            text.push_str(&serde_json::to_string(&r).unwrap());
            text.push('\n');
        }
        fs::write(&path, text).unwrap();
        append(&path, &record(count as u64)).unwrap();
        let kept = read(&path);
        assert_eq!(kept.len(), KEEP);
        assert_eq!(kept.last().unwrap().timestamp_ms, count as u64);
        assert_eq!(kept[0].timestamp_ms, (count + 1 - KEEP) as u64);
    }

    #[test]
    fn a_record_has_no_field_that_could_hold_a_secret_or_text() {
        let json = serde_json::to_value(record(1)).unwrap();
        let names: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        for forbidden in ["key", "authorization", "audio", "text", "transcript", "body"] {
            assert!(
                names.iter().all(|name| !name.contains(forbidden) || name.starts_with("audio_ms")),
                "{forbidden} in {names:?}"
            );
        }
    }
}
