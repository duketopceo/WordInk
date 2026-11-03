//! "Copy diagnostics": a plain-text summary a person can paste into a bug
//! report. It is built only when asked for (the Privacy pane's Copy
//! diagnostics row, or `wordink diagnostics`) and is never sent anywhere.
//!
//! It names the app version, update channel, OS, architecture, engine kind,
//! install method, how the cloud engines performed (counts and timings, no
//! content) and the last few failure messages the event log already
//! keeps. It never holds audio, transcript text, file paths (the home folder
//! included), user names or keys: failure messages are scrubbed of anything
//! that looks like one, and the transcripts in the event log are never read.

use std::path::Path;

use crate::desktop::events::{CommandOutcome, DictationPhase, EventReader, VoiceEvent};
use crate::history::usage;
use crate::transcription::cloud::Provider;
use crate::transcription::cloud::request_log::{self, RequestRecord};

const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// The cloud request log's records, or none when there is no log.
pub fn recent_requests() -> Vec<RequestRecord> {
    request_log::default_path()
        .map(|path| request_log::read(&path))
        .unwrap_or_default()
}

/// How many recent failures the summary lists.
const RECENT_ERRORS: usize = 5;
/// The longest a listed failure message gets, in characters.
const MAX_MESSAGE_CHARS: usize = 160;
/// The longest a scrubbed debug-log line gets, in characters.
const MAX_LINE_CHARS: usize = 1200;

/// Everything the summary says, already safe to print.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostics {
    pub version: String,
    pub channel: String,
    pub os: String,
    pub arch: String,
    pub engine: String,
    pub install: String,
    /// One line per cloud engine used in the last week: p50 and p95
    /// transcription time, real-time factor, error rate and cost per audio
    /// hour. Numbers only.
    pub cloud: Vec<String>,
    pub errors: Vec<String>,
    /// Why the event log could not be read (the error kind, never a path);
    /// the summary then says so instead of claiming there were no errors.
    pub log_unreadable: Option<String>,
}

/// The engine kind in words: `local`, `Groq`, or `cloud (<provider>)`.
pub fn engine_kind(provider: Option<Provider>) -> String {
    match provider {
        None => "local".to_owned(),
        Some(Provider::Groq) => "Groq".to_owned(),
        Some(other) => format!("cloud ({})", other.name()),
    }
}

/// What to strip from a failure message: the home folder and user name of
/// this machine.
#[derive(Clone, Debug, Default)]
pub struct Redactions {
    pub home: Option<String>,
    pub user: Option<String>,
}

impl Redactions {
    /// This process's home folder and user name, from the environment.
    pub fn from_environment() -> Self {
        let var = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| std::env::var(name).ok())
                .filter(|value| !value.trim().is_empty())
        };
        Self {
            home: var(&["HOME", "USERPROFILE"]),
            user: var(&["USER", "USERNAME", "LOGNAME"]),
        }
    }
}

impl Diagnostics {
    /// The summary for this running install. `provider` is the cloud engine
    /// dictation uses now (`None` is local); `event_log` is `live.ndjson`.
    pub fn collect(provider: Option<Provider>, event_log: Option<&Path>) -> Self {
        let (channel, install) = crate::updater::install_summary();
        let read = event_log.map(|path| recent_errors(path, &Redactions::from_environment()));
        let (errors, log_unreadable) = match read {
            None => (Vec::new(), None),
            Some(Ok(errors)) => (errors, None),
            Some(Err(kind)) => (Vec::new(), Some(kind.to_string())),
        };
        Self {
            version: crate::VERSION.to_owned(),
            channel,
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            engine: engine_kind(provider),
            install,
            cloud: Vec::new(),
            errors,
            log_unreadable,
        }
    }

    /// Adds the cloud engines' timing lines from the request log's records
    /// of the last week.
    pub fn with_requests(mut self, requests: &[RequestRecord]) -> Self {
        let since = crate::history::now_ms().saturating_sub(WEEK_MS);
        self.cloud = usage::request_latency(requests, None, since)
            .iter()
            .map(usage::latency_line)
            .collect();
        self
    }

    /// The plain text people copy.
    pub fn text(&self) -> String {
        let mut text = format!(
            "WordInk diagnostics\nVersion: {}\nUpdate channel: {}\nOS: {}\nArchitecture: {}\nEngine: {}\nInstall method: {}\n",
            self.version, self.channel, self.os, self.arch, self.engine, self.install
        );
        if !self.cloud.is_empty() {
            text.push_str("Cloud requests, last 7 days:\n");
            for line in &self.cloud {
                text.push_str("- ");
                text.push_str(line);
                text.push('\n');
            }
        }
        if let Some(kind) = &self.log_unreadable {
            text.push_str(&format!(
                "Recent errors: unavailable (could not read the event log: {kind})\n"
            ));
        } else if self.errors.is_empty() {
            text.push_str("Recent errors: none\n");
        } else {
            text.push_str("Recent errors:\n");
            for error in &self.errors {
                text.push_str("- ");
                text.push_str(error);
                text.push('\n');
            }
        }
        text
    }
}

/// The newest failures in the event log, newest first, scrubbed. Only the
/// failure messages are read: transcripts, commands heard and pasted text in
/// the same log never reach the summary.
fn recent_errors(path: &Path, redactions: &Redactions) -> Result<Vec<String>, std::io::ErrorKind> {
    let mut reader = EventReader::open(path);
    reader.refresh().map_err(|error| error.kind())?;
    let mut errors: Vec<String> = Vec::new();
    for event in reader.events().iter().rev() {
        let message = match event {
            VoiceEvent::Dictation {
                phase: DictationPhase::Failed(message),
                ..
            } => format!("dictation: {message}"),
            VoiceEvent::Command {
                outcome: CommandOutcome::Failed(message),
                ..
            } => format!("command: {message}"),
            VoiceEvent::ApiAuthFailed { .. } => {
                "local API: a request failed to authenticate".to_owned()
            }
            _ => continue,
        };
        let message = scrub(&message, redactions);
        if errors.last() != Some(&message) {
            errors.push(message);
        }
        if errors.len() == RECENT_ERRORS {
            break;
        }
    }
    Ok(errors)
}

/// `message` on one line with paths, key-like tokens and this machine's user
/// name replaced, cut to a short length.
pub fn scrub(message: &str, redactions: &Redactions) -> String {
    scrub_to(message, redactions, MAX_MESSAGE_CHARS)
}

/// [`scrub`] for one debug-log line, which is longer than a failure message.
pub fn scrub_line(line: &str, redactions: &Redactions) -> String {
    scrub_to(line, redactions, MAX_LINE_CHARS)
}

fn scrub_to(message: &str, redactions: &Redactions, limit: usize) -> String {
    let message = strip_machine(message, redactions);
    let words: Vec<String> = message
        .split_whitespace()
        .map(|word| {
            if looks_like_path(word) {
                "<path>".to_owned()
            } else if word_is_key(word) {
                "<redacted>".to_owned()
            } else {
                word.to_owned()
            }
        })
        .collect();
    let line = words.join(" ");
    if line.chars().count() > limit {
        let cut: String = line.chars().take(limit).collect();
        format!("{cut}...")
    } else {
        line
    }
}

/// `line` with this machine's home folder and user name, and any key-like
/// token (bare or as a `name=value` value), replaced. Spacing and paths stay,
/// so a log line keeps its shape; the debug log writes through this.
pub fn redact(line: &str, redactions: &Redactions) -> String {
    strip_machine(line, redactions)
        .split(' ')
        .map(|word| {
            if word_is_key(word) {
                "<redacted>"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_machine(message: &str, redactions: &Redactions) -> String {
    let mut message = message.to_owned();
    // Longest first, so the home folder goes before the user name inside it.
    for secret in [redactions.home.as_deref(), redactions.user.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| value.chars().count() >= 2 && *value != "/")
    {
        message = replace_ignoring_case(&message, secret, "<redacted>");
    }
    message
}

/// A word that is a key, or `name=key`.
fn word_is_key(word: &str) -> bool {
    looks_like_key(word)
        || word
            .split_once('=')
            .is_some_and(|(_, value)| looks_like_key(value))
}

fn replace_ignoring_case(haystack: &str, needle: &str, with: &str) -> String {
    // Compare by lowercased characters, so scripts whose lowercase form has a
    // different byte length (Turkish İ, for one) are still matched in any case.
    let target: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if target.is_empty() {
        return haystack.to_owned();
    }
    let chars: Vec<char> = haystack.chars().collect();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < chars.len() {
        let mut lowered: Vec<char> = Vec::with_capacity(target.len());
        let mut j = i;
        while j < chars.len() && lowered.len() < target.len() {
            lowered.extend(chars[j].to_lowercase());
            j += 1;
        }
        if lowered == target {
            out.push_str(with);
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// A word that names a file: it holds a path separator, starts with `~`, or
/// is a Windows drive path. Quotes and trailing punctuation are ignored.
fn looks_like_path(word: &str) -> bool {
    let word =
        word.trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')' | ',' | ';' | ':'));
    let bytes = word.as_bytes();
    word.contains('/')
        || word.contains('\\')
        || word.starts_with('~')
        || (bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && word.len() > 2)
}

/// A word shaped like an API key: a known prefix, or a long unbroken run of
/// letters and digits with at least one of each.
fn looks_like_key(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    const PREFIXES: [&str; 6] = ["gsk_", "sk-", "sk_", "pk_", "dg_", "ghp_"];
    let lower = word.to_ascii_lowercase();
    if PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) && word.len() >= 8 {
        return true;
    }
    word.len() >= 24
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && word.chars().any(|c| c.is_ascii_digit())
        && word.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/kit";
    const KEY: &str = "gsk_window_secret_0123456789";
    const TRANSCRIPT: &str = "my private dictated sentence about the merger";

    fn redactions() -> Redactions {
        Redactions {
            home: Some(HOME.into()),
            user: Some("kit".into()),
        }
    }

    fn fixture_log() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wordink-diagnostics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live.ndjson");
        let lines = [
            r#"{"kind":"session_started","timestamp_ms":1}"#.to_owned(),
            format!(
                r#"{{"kind":"transcript","timestamp_ms":2,"phase":"completed","latency_ms":5,"text":"{TRANSCRIPT}"}}"#
            ),
            format!(
                r#"{{"kind":"dictation","timestamp_ms":3,"phase":"pasted","text":"{TRANSCRIPT}"}}"#
            ),
            format!(
                r#"{{"kind":"dictation","timestamp_ms":4,"phase":{{"failed":"cloud request failed for {HOME}/.config/wordink/clip.wav with {KEY}"}}}}"#
            ),
            format!(
                r#"{{"kind":"command","timestamp_ms":5,"heard":"{TRANSCRIPT}","command":"x","outcome":{{"failed":"paste helper exited 1"}}}}"#
            ),
        ];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        path
    }

    fn state(errors: Vec<String>) -> Diagnostics {
        Diagnostics {
            version: "1.0.0".into(),
            channel: "stable".into(),
            os: "linux".into(),
            arch: "aarch64".into(),
            engine: engine_kind(Some(Provider::Groq)),
            install: "direct".into(),
            cloud: Vec::new(),
            errors,
            log_unreadable: None,
        }
    }

    #[test]
    fn the_text_names_version_channel_platform_engine_and_install() {
        let text = state(Vec::new()).text();
        for line in [
            "Version: 1.0.0",
            "Update channel: stable",
            "OS: linux",
            "Architecture: aarch64",
            "Engine: Groq",
            "Install method: direct",
            "Recent errors: none",
        ] {
            assert!(text.contains(line), "{line} missing from:\n{text}");
        }
    }

    #[test]
    fn engine_kinds_are_local_groq_or_the_provider_name() {
        assert_eq!(engine_kind(None), "local");
        assert_eq!(engine_kind(Some(Provider::Groq)), "Groq");
        assert_eq!(engine_kind(Some(Provider::OpenAi)), "cloud (OpenAI)");
    }

    #[test]
    fn recent_errors_are_listed_without_transcripts_paths_or_keys() {
        let path = fixture_log();
        let errors = recent_errors(&path, &redactions()).unwrap();
        let text = state(errors.clone()).text();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(text.contains("command: paste helper exited 1"));
        assert!(text.contains("dictation: cloud request failed for <path> with <redacted>"));
        for secret in [TRANSCRIPT, HOME, "kit", KEY, "clip.wav", "merger"] {
            assert!(
                !text.contains(secret),
                "a fixture value leaked into the diagnostics"
            );
        }
        assert!(text.contains("Recent errors:\n- "));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_event_log_means_no_recent_errors() {
        let missing = std::env::temp_dir().join("wordink-diagnostics-none/live.ndjson");
        assert!(recent_errors(&missing, &redactions()).unwrap().is_empty());
        assert!(state(Vec::new()).text().contains("Recent errors: none"));
    }

    #[test]
    fn an_unreadable_event_log_is_reported_not_called_none() {
        // A directory where the log should be: opening it as a file fails
        // with something other than NotFound.
        let dir =
            std::env::temp_dir().join(format!("wordink-diag-unreadable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(recent_errors(&dir, &redactions()).is_err());
        let mut unreadable = state(Vec::new());
        unreadable.log_unreadable = Some("permission denied".into());
        let text = unreadable.text();
        assert!(text.contains("Recent errors: unavailable"), "{text}");
        assert!(!text.contains("Recent errors: none"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scrub_removes_the_home_folder_user_name_paths_and_keys() {
        let scrubbed = scrub(
            "could not open /home/kit/notes.txt, C:\\Users\\Kit\\a.wav and ~/x as KIT: sk-abc12345678 abcdefghijklmnopqrstuvwxyz0123",
            &redactions(),
        );
        for secret in [
            "kit",
            "Kit",
            "KIT",
            "notes",
            "a.wav",
            "sk-abc",
            "abcdefghijkl",
        ] {
            assert!(
                !scrubbed.contains(secret),
                "a fixture value survived scrubbing"
            );
        }
        assert!(scrub("microphone busy", &redactions()) == "microphone busy");
        assert!(scrub(&"x ".repeat(300), &redactions()).chars().count() <= MAX_MESSAGE_CHARS + 3);
    }

    #[test]
    fn names_are_scrubbed_in_any_case_even_when_lowercasing_changes_length() {
        let scrubbed = replace_ignoring_case("İ: émile ÉMILE Émile done", "Émile", "<user>");
        assert_eq!(scrubbed, "İ: <user> <user> <user> done");
        assert_eq!(
            replace_ignoring_case("kit KIT Kit", "kit", "<user>"),
            "<user> <user> <user>"
        );
        assert_eq!(
            replace_ignoring_case("unchanged", "", "<user>"),
            "unchanged"
        );
    }

    #[test]
    fn plain_words_are_not_mistaken_for_keys() {
        for word in ["transcription", "microphone", "HTTP", "401", "Unauthorized"] {
            assert!(!looks_like_key(word) && !looks_like_path(word), "{word}");
        }
    }

    #[test]
    fn cloud_timings_are_numbers_only_and_listed_after_the_engine() {
        let now = crate::history::now_ms();
        let record = |latency_ms, ok: bool| RequestRecord {
            timestamp_ms: now - 1_000,
            provider: "openrouter".into(),
            endpoint_kind: "transcription".into(),
            model: "microsoft/mai-transcribe-2".into(),
            audio_ms: 5_000,
            latency_ms,
            error_class: (!ok).then(|| "unavailable".to_owned()),
            ..RequestRecord::default()
        };
        let old = RequestRecord {
            timestamp_ms: now - 8 * 24 * 60 * 60 * 1000,
            ..record(1, true)
        };
        let text = state(Vec::new())
            .with_requests(&[record(4_000, true), record(6_000, true), record(900, false), old])
            .text();
        assert!(
            text.contains("Cloud requests, last 7 days:\n- openrouter \u{b7} microsoft/mai-transcribe-2: p50 5.0 s, p95 5.9 s"),
            "{text}"
        );
        assert!(text.contains("33% errors (1 of 3 requests)"), "{text}");
        assert!(text.find("Engine:").unwrap() < text.find("Cloud requests").unwrap());
        // No requests, no section.
        assert!(!state(Vec::new()).with_requests(&[]).text().contains("Cloud requests"));
    }
}
