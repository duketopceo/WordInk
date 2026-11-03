//! Debug logs: what every stage of a dictation did and how long it took, for
//! diagnosing problems such as a microphone that records silence or earbuds
//! that never come back.
//!
//! Events are always captured, silently, into a bounded in-memory ring
//! ([`Ring`]: the last 5,000 events or about 2 MB). Nothing is formatted and
//! no file is touched until someone looks: the live log view, `wordink logs
//! --debug`, or Copy recent debug log. Developer mode (`developer_mode`)
//! additionally persists events to `debug.log` in the logs folder (rotated,
//! 5 MB x 3) and opens the live view.
//!
//! Events never hold keys, auth headers or audio: callers pass counts and
//! names, and the transcript is withheld unless `debug_logging_transcripts`
//! is also on ([`transcript`]). Everything rendered is redacted again
//! ([`format_text`]) and anything copied out is scrubbed ([`copy_text`]).
//!
//! One event is one line: `2026-11-02T10:15:03.123Z +4821ms DEBUG audio.route
//! message key=value`, or a JSON object with `--json`. `tracing` events from
//! Hex's own modules join through [`BridgeLayer`].

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::Redactions;

/// The file name of the live debug log in the logs folder.
pub const LOG_NAME: &str = "debug.log";
/// A log is rotated when it would pass this size.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// How many files rotation keeps, the live one included.
pub const KEEP_FILES: usize = 3;
/// The most lines "Copy recent debug log" copies.
pub const COPY_LINES: usize = 400;
/// The ring keeps at most this many events...
pub const RING_EVENTS: usize = 5_000;
/// ...and at most about this many bytes of them.
pub const RING_BYTES: usize = 2 * 1024 * 1024;
/// The subsystems events are filed under (others, such as `app`, also occur).
pub const SUBSYSTEMS: [&str; 9] = [
    "audio",
    "bluetooth",
    "vad",
    "engine",
    "output",
    "media",
    "hotkeys",
    "settings",
    "updater",
];

/// How much the debug log records. Each level includes the one before it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugLevel {
    /// Decisions and failures: what was chosen, what ended a capture and why.
    #[default]
    Info,
    /// Plus timings, device inventories and each step's result.
    Debug,
    /// Plus per-chunk audio levels and every tracing event.
    Trace,
}

impl DebugLevel {
    pub const ALL: [DebugLevel; 3] = [DebugLevel::Info, DebugLevel::Debug, DebugLevel::Trace];

    pub fn label(self) -> &'static str {
        match self {
            DebugLevel::Info => "Info",
            DebugLevel::Debug => "Debug",
            DebugLevel::Trace => "Trace",
        }
    }

    fn name(self) -> &'static str {
        match self {
            DebugLevel::Info => "INFO",
            DebugLevel::Debug => "DEBUG",
            DebugLevel::Trace => "TRACE",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }
}

/// One thing that happened, kept unformatted until someone reads it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Event {
    /// Counts events from 1 in this run; the cursor `logs --follow` resumes from.
    pub seq: u64,
    /// Milliseconds since this process started: monotonic.
    pub mono_ms: u64,
    /// Milliseconds since the Unix epoch.
    pub wall_ms: u64,
    pub level: DebugLevel,
    pub subsystem: String,
    pub name: String,
    pub message: String,
    pub fields: Vec<(String, Value)>,
}

impl Event {
    fn size(&self) -> usize {
        let value = |value: &Value| match value {
            Value::String(text) => text.len(),
            Value::Null | Value::Bool(_) | Value::Number(_) => 16,
            other => other.to_string().len(),
        };
        96 + self.subsystem.len()
            + self.name.len()
            + self.message.len()
            + self
                .fields
                .iter()
                .map(|(key, field)| 32 + key.len() + value(field))
                .sum::<usize>()
    }
}

/// Which events a view, a copy or a CLI follow shows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Filter {
    /// Shows events at this level and below (`Info` shows only info).
    pub level: DebugLevel,
    /// Shows only these subsystems; empty shows all.
    pub subsystems: Vec<String>,
    /// Shows only events whose line contains this, ignoring case.
    pub search: String,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            level: DebugLevel::Trace,
            subsystems: Vec::new(),
            search: String::new(),
        }
    }
}

impl Filter {
    pub fn matches(&self, event: &Event) -> bool {
        event.level <= self.level
            && (self.subsystems.is_empty() || self.subsystems.contains(&event.subsystem))
            && (self.search.is_empty()
                || format_text(event, &Redactions::default())
                    .to_lowercase()
                    .contains(&self.search.to_lowercase()))
    }
}

/// The bounded, in-memory log. Oldest events go first, past either bound.
pub struct Ring {
    events: VecDeque<Event>,
    bytes: usize,
    next_seq: u64,
    max_events: usize,
    max_bytes: usize,
    subscribers: Vec<Sender<Event>>,
}

impl Ring {
    pub const fn new(max_events: usize, max_bytes: usize) -> Self {
        Self {
            events: VecDeque::new(),
            bytes: 0,
            next_seq: 1,
            max_events,
            max_bytes,
            subscribers: Vec::new(),
        }
    }

    /// Adds `event`, numbering it and handing it to every live subscriber.
    pub fn push(&mut self, mut event: Event) {
        event.seq = self.next_seq;
        self.next_seq += 1;
        self.subscribers
            .retain(|subscriber| subscriber.send(event.clone()).is_ok());
        self.bytes += event.size();
        self.events.push_back(event);
        while self.events.len() > self.max_events
            || (self.bytes > self.max_bytes && self.events.len() > 1)
        {
            if let Some(oldest) = self.events.pop_front() {
                self.bytes -= oldest.size();
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.bytes = 0;
    }

    /// The events after `after` that `filter` shows, oldest first, and the
    /// cursor to resume from. With no cursor (`after` 0) it returns the
    /// newest `limit` of the history; later it returns the next `limit`.
    pub fn since(&self, after: u64, filter: &Filter, limit: usize) -> (Vec<Event>, u64) {
        let latest = self.next_seq - 1;
        let mut found: Vec<Event> = self
            .events
            .iter()
            .filter(|event| event.seq > after && filter.matches(event))
            .cloned()
            .collect();
        if found.len() <= limit {
            return (found, latest.max(after));
        }
        if after == 0 {
            found.drain(..found.len() - limit);
            (found, latest)
        } else {
            found.truncate(limit);
            let next = found.last().map_or(after, |event| event.seq);
            (found, next)
        }
    }

    /// Everything buffered, and a receiver for every event after it: no gap
    /// and no repeat between the two.
    pub fn subscribe(&mut self) -> (Vec<Event>, Receiver<Event>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.subscribers.push(sender);
        (self.events.iter().cloned().collect(), receiver)
    }
}

/// The level events are persisted at, plus whether developer mode persists.
static PERSIST: AtomicBool = AtomicBool::new(false);
static FILE_LEVEL: AtomicU8 = AtomicU8::new(0);
/// Trace events are recorded only when the level is trace.
static TRACE_ON: AtomicBool = AtomicBool::new(false);
static TRANSCRIPTS: AtomicBool = AtomicBool::new(false);
static RING: Mutex<Ring> = Mutex::new(Ring::new(RING_EVENTS, RING_BYTES));
static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();
static REDACTIONS: OnceLock<Redactions> = OnceLock::new();

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Applies the saved settings. `developer` persists events to the file;
/// `level` bounds what the file takes and whether trace events are captured
/// at all. Unit tests never switch the globals on, so no test writes into a
/// real logs folder.
pub fn configure(developer: bool, level: DebugLevel, transcripts: bool) {
    START.get_or_init(Instant::now);
    if cfg!(test) {
        return;
    }
    let was_on = PERSIST.load(Ordering::Relaxed);
    TRANSCRIPTS.store(transcripts, Ordering::Relaxed);
    TRACE_ON.store(level == DebugLevel::Trace, Ordering::Relaxed);
    FILE_LEVEL.store(level as u8, Ordering::Relaxed);
    PERSIST.store(developer, Ordering::Relaxed);
    if developer && !was_on {
        log(
            DebugLevel::Info,
            "app.developer_mode",
            "developer mode on: persisting debug logs",
            &[
                ("level", val(&level)),
                ("transcripts", val(&transcripts)),
                ("version", val(env!("CARGO_PKG_VERSION"))),
                ("os", val(std::env::consts::OS)),
            ],
        );
    } else if !developer && was_on {
        log(
            DebugLevel::Info,
            "app.developer_mode",
            "developer mode off",
            &[],
        );
        // Close the file so "Clear debug logs" can remove it everywhere.
        *lock(&SINK) = None;
    }
}

/// Whether an event at `level` is captured now: everything but trace, which
/// needs the trace level.
pub fn enabled(level: DebugLevel) -> bool {
    level < DebugLevel::Trace || TRACE_ON.load(Ordering::Relaxed)
}

/// `text` when transcripts are opted in, else a stand-in that says only how
/// long it was.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn transcript(text: &str) -> String {
    if TRANSCRIPTS.load(Ordering::Relaxed) {
        text.to_owned()
    } else {
        format!("<withheld {} chars>", text.chars().count())
    }
}

/// A field value: anything that serializes.
pub fn val<T: Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Records one event: `dlog!(Debug, "audio.open", "opened", device = id, ms = 12)`.
/// The stage is `subsystem.event`.
macro_rules! dlog {
    ($level:ident, $stage:expr, $message:expr $(, $key:ident = $value:expr)* $(,)?) => {
        if $crate::debug_log::enabled($crate::debug_log::DebugLevel::$level) {
            $crate::debug_log::log(
                $crate::debug_log::DebugLevel::$level,
                $stage,
                $message,
                &[$((stringify!($key), $crate::debug_log::val(&$value))),*],
            );
        }
    };
}
pub(crate) use dlog;

fn mono_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// Captures one event. Call through [`dlog!`](crate::debug_log::dlog), which
/// checks the level first.
pub fn log(level: DebugLevel, stage: &str, message: &str, fields: &[(&str, Value)]) {
    if !enabled(level) {
        return;
    }
    let (subsystem, name) = stage.split_once('.').unwrap_or((stage, ""));
    let event = Event {
        seq: 0,
        mono_ms: mono_ms(),
        wall_ms: wall_ms(),
        level,
        subsystem: subsystem.to_owned(),
        name: name.to_owned(),
        message: message.to_owned(),
        fields: fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    };
    // Formatting and file I/O happen only while developer mode persists.
    let line = (PERSIST.load(Ordering::Relaxed)
        && (level as u8) <= FILE_LEVEL.load(Ordering::Relaxed))
    .then(|| format_text(&event, redactions()));
    lock(&RING).push(event);
    if let Some(line) = line {
        write_to_file(&line);
    }
}

fn redactions() -> &'static Redactions {
    REDACTIONS.get_or_init(Redactions::from_environment)
}

fn write_to_file(line: &str) {
    let mut sink = lock(&SINK);
    if sink.is_none() {
        let Ok(dir) = crate::app_paths::logs_dir() else {
            return;
        };
        *sink = Sink::open(&dir, MAX_BYTES, KEEP_FILES).ok();
    }
    if let Some(sink) = sink.as_mut() {
        sink.write_line(line);
    }
}

/// The buffered events `filter` shows after `after`, and the next cursor.
pub fn since(after: u64, filter: &Filter, limit: usize) -> (Vec<Event>, u64) {
    lock(&RING).since(after, filter, limit)
}

/// Everything buffered, then a receiver for each event after it.
pub fn subscribe() -> (Vec<Event>, Receiver<Event>) {
    lock(&RING).subscribe()
}

/// One event as a line: `2026-11-02T10:15:03.123Z +4821ms DEBUG audio.route
/// message key=value`. Keys, this machine's home folder and its user name
/// are replaced even if a caller passed one by mistake.
pub fn format_text(event: &Event, redactions: &Redactions) -> String {
    let stage = if event.name.is_empty() {
        event.subsystem.clone()
    } else {
        format!("{}.{}", event.subsystem, event.name)
    };
    let mut line = format!(
        "{} +{}ms {} {stage} {}",
        rfc3339(event.wall_ms),
        event.mono_ms,
        event.level.name(),
        one_line(&event.message)
    );
    for (key, value) in &event.fields {
        line.push(' ');
        line.push_str(key);
        line.push('=');
        match value {
            Value::String(text) => line.push_str(&quoted(text)),
            Value::Null => line.push_str("null"),
            other => line.push_str(&quoted(&other.to_string())),
        }
    }
    crate::diagnostics::redact(&line, redactions)
}

/// One event as a JSON object on one line, redacted like [`format_text`].
pub fn format_json(event: &Event, redactions: &Redactions) -> String {
    let clean = |text: &str| crate::diagnostics::redact(text, redactions);
    let fields: serde_json::Map<String, Value> = event
        .fields
        .iter()
        .map(|(key, value)| {
            let value = match value {
                Value::String(text) => Value::String(clean(text)),
                // Words inside a nested value are redacted through its text.
                Value::Array(_) | Value::Object(_) => Value::String(clean(&value.to_string())),
                other => other.clone(),
            };
            (key.clone(), value)
        })
        .collect();
    serde_json::json!({
        "seq": event.seq,
        "ts": rfc3339(event.wall_ms),
        "wall_ms": event.wall_ms,
        "mono_ms": event.mono_ms,
        "level": event.level.name().to_ascii_lowercase(),
        "subsystem": event.subsystem,
        "event": event.name,
        "message": clean(&event.message),
        "fields": fields,
    })
    .to_string()
}

/// `events` as text for a bug report: each line scrubbed of home folders,
/// user names, paths and key-like tokens.
pub fn copy_text(events: &[Event], redactions: &Redactions) -> String {
    events
        .iter()
        .map(|event| crate::diagnostics::scrub_line(&format_text(event, redactions), redactions))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The last `lines` buffered events that `filter` shows, as [`copy_text`].
pub fn recent(lines: usize, filter: &Filter) -> Result<String, String> {
    let (mut events, _) = since(0, filter, lines);
    if events.is_empty() {
        return Err("there is no debug log yet".into());
    }
    events.truncate(lines);
    Ok(copy_text(&events, &Redactions::from_environment()))
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// A value bare when plain, else in quotes with its quotes and newlines escaped.
fn quoted(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| !c.is_whitespace() && c != '"' && c != '=' && !c.is_control());
    if plain {
        text.to_owned()
    } else {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "");
        format!("\"{escaped}\"")
    }
}

/// UTC time in RFC 3339 with milliseconds, without a date-time dependency.
fn rfc3339(at_ms: u64) -> String {
    let seconds = at_ms / 1000;
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        at_ms % 1000
    )
}

/// The live log file, rotated when it would pass `max_bytes`; the rotated
/// files are `debug.log.1` (newest) up to `keep - 1`.
pub struct Sink {
    dir: PathBuf,
    file: File,
    size: u64,
    max_bytes: u64,
    keep: usize,
}

impl Sink {
    pub fn open(dir: &Path, max_bytes: u64, keep: usize) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(LOG_NAME))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_owned(),
            file,
            size,
            max_bytes,
            keep: keep.max(1),
        })
    }

    pub fn write_line(&mut self, line: &str) {
        let length = line.len() as u64 + 1;
        if self.size > 0 && self.size + length > self.max_bytes {
            self.rotate();
        }
        if writeln!(self.file, "{line}").is_ok() {
            self.size += length;
        }
    }

    fn rotate(&mut self) {
        let numbered = |n: usize| self.dir.join(format!("{LOG_NAME}.{n}"));
        if self.keep > 1 {
            let _ = std::fs::remove_file(numbered(self.keep - 1));
            for n in (1..self.keep - 1).rev() {
                let _ = std::fs::rename(numbered(n), numbered(n + 1));
            }
            let _ = std::fs::rename(self.dir.join(LOG_NAME), numbered(1));
        }
        // With one file kept, truncating starts it over.
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.dir.join(LOG_NAME))
        {
            self.file = file;
            self.size = 0;
        }
    }
}

/// The debug log files in `dir`, newest first.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![dir.join(LOG_NAME)];
    files.extend((1..KEEP_FILES).map(|n| dir.join(format!("{LOG_NAME}.{n}"))));
    files.retain(|path| path.exists());
    files
}

/// Deletes the debug log and its rotated files in `dir`; how many went.
pub fn clear_in(dir: &Path) -> usize {
    files_in(dir)
        .into_iter()
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count()
}

/// Empties the buffer and deletes this install's debug log files.
pub fn clear() -> Result<usize, String> {
    let dir = crate::app_paths::logs_dir().map_err(|error| format!("{error:#}"))?;
    lock(&RING).clear();
    let mut sink = lock(&SINK);
    // Release the open file first so every OS can delete it.
    *sink = None;
    Ok(clear_in(&dir))
}

/// Where the debug log is written, for Settings and the CLI to show.
pub fn path() -> Option<PathBuf> {
    crate::app_paths::logs_dir()
        .ok()
        .map(|dir| dir.join(LOG_NAME))
}

/// Feeds `tracing` events from Hex's own modules into the debug log.
pub struct BridgeLayer;

/// The subsystem a module's events are filed under.
fn subsystem_of(target: &str) -> &'static str {
    let module = target.trim_start_matches("hex::");
    let has = |parts: &[&str]| parts.iter().any(|part| module.contains(part));
    if has(&["headset", "bluetooth"]) {
        "bluetooth"
    } else if has(&["audio_control", "browser_media", "suppress"]) {
        "media"
    } else if has(&["speech_detector", "voice_filter"]) {
        "vad"
    } else if has(&["transcription", "recognition", "parakeet", "gguf"]) {
        "engine"
    } else if has(&["paste", "clipboard", "portal", "remote_desktop"]) {
        "output"
    } else if has(&["linux::input", "hotkey", "shortcut"]) {
        "hotkeys"
    } else if has(&["app_settings", "settings"]) {
        "settings"
    } else if has(&["updater"]) {
        "updater"
    } else if has(&["audio", "pulse", "input_route"]) {
        "audio"
    } else {
        "app"
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for BridgeLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        let meta = event.metadata();
        let level = match *meta.level() {
            tracing::Level::ERROR | tracing::Level::WARN | tracing::Level::INFO => DebugLevel::Info,
            tracing::Level::DEBUG => DebugLevel::Debug,
            tracing::Level::TRACE => DebugLevel::Trace,
        };
        if !enabled(level) {
            return;
        }
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let stage = format!(
            "{}.{}",
            subsystem_of(meta.target()),
            meta.level().as_str().to_ascii_lowercase()
        );
        let mut fields: Vec<(&str, Value)> = vec![(
            "module",
            Value::String(meta.target().trim_start_matches("hex::").to_owned()),
        )];
        fields.extend(
            visitor
                .fields
                .iter()
                .map(|(key, value)| (key.as_str(), Value::String(value.clone()))),
        );
        log(level, &stage, &visitor.message, &fields);
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields
                .push((field.name().to_owned(), format!("{value:?}")));
        }
    }
}

/// Whether the [`BridgeLayer`] takes an event: Hex's own, and trace ones
/// only at the trace level.
pub fn bridge_wants(meta: &tracing::Metadata<'_>) -> bool {
    meta.target().starts_with("hex")
        && (*meta.level() != tracing::Level::TRACE || TRACE_ON.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests;
