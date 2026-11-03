//! "Copy diagnostics": a plain-text summary a person can paste into a bug
//! report. It is built only when asked for (the Privacy pane's Copy
//! diagnostics row, or `wordink diagnostics`) and is never sent anywhere.
//!
//! It names the app version, update channel, OS, architecture, engine kind,
//! install method and the last few failure messages the event log already
//! keeps. It never holds audio, transcript text, file paths (the home folder
//! included), user names or keys: failure messages are scrubbed of anything
//! that looks like one, and the transcripts in the event log are never read.

use std::path::Path;

use crate::desktop::events::{CommandOutcome, DictationPhase, EventReader, VoiceEvent};
use crate::transcription::cloud::Provider;

/// How many recent failures the summary lists.
const RECENT_ERRORS: usize = 5;
/// The longest a listed failure message gets, in characters.
const MAX_MESSAGE_CHARS: usize = 160;

/// Everything the summary says, already safe to print.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostics {
    pub version: String,
    pub channel: String,
    pub os: String,
    pub arch: String,
    pub engine: String,
    pub install: String,
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
            errors,
            log_unreadable,
        }
    }

    /// The plain text people copy.
    pub fn text(&self) -> String {
        let mut text = format!(
            "WordInk diagnostics\nVersion: {}\nUpdate channel: {}\nOS: {}\nArchitecture: {}\nEngine: {}\nInstall method: {}\n",
            self.version, self.channel, self.os, self.arch, self.engine, self.install
        );
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
    let mut message = message.to_owned();
    // Longest first, so the home folder goes before the user name inside it.
    for secret in [redactions.home.as_deref(), redactions.user.as_deref()]
        .into_iter()
        .flatten()
        .filter(|value| value.chars().count() >= 2 && *value != "/")
    {
        message = replace_ignoring_case(&message, secret, "<redacted>");
    }
    let words: Vec<String> = message
        .split_whitespace()
        .map(|word| {
            if looks_like_path(word) {
                "<path>".to_owned()
            } else if looks_like_key(word) {
                "<redacted>".to_owned()
            } else {
                word.to_owned()
            }
        })
        .collect();
    let line = words.join(" ");
    if line.chars().count() > MAX_MESSAGE_CHARS {
        let cut: String = line.chars().take(MAX_MESSAGE_CHARS).collect();
        format!("{cut}...")
    } else {
        line
    }
}

fn replace_ignoring_case(haystack: &str, needle: &str, with: &str) -> String {
    let (lower, needle) = (haystack.to_lowercase(), needle.to_lowercase());
    // Lowercasing can change byte lengths for some scripts; fall back to an
    // exact replace then.
    if lower.len() != haystack.len() {
        return haystack.replace(needle.as_str(), with);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut rest = 0;
    while let Some(found) = lower[rest..].find(&needle) {
        out.push_str(&haystack[rest..rest + found]);
        out.push_str(with);
        rest += found + needle.len();
    }
    out.push_str(&haystack[rest..]);
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
    fn plain_words_are_not_mistaken_for_keys() {
        for word in ["transcription", "microphone", "HTTP", "401", "Unauthorized"] {
            assert!(!looks_like_key(word) && !looks_like_path(word), "{word}");
        }
    }
}
