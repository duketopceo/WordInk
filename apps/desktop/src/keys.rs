//! Provider API keys for the cloud engines.
//!
//! Resolution order, first hit wins:
//!
//! 1. the provider's environment variable (`GROQ_API_KEY`);
//! 2. the OS keyring: Secret Service on Linux, Keychain on macOS, Credential
//!    Manager on Windows;
//! 3. a user-only (0600) file named for the provider (`groq.key`) beside
//!    `settings.json`, for Linux sessions with no Secret Service.
//!
//! A key is never written to the settings file or to a log, and no function
//! here returns one for display: [`status`] says only where a key was found.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

const SERVICE: &str = "wordink";
/// A keyring call that has not answered by now counts as unavailable, so a
/// session with no Secret Service never stalls the hotkey loop.
const KEYRING_DEADLINE: Duration = Duration::from_secs(2);
/// After the keyring fails or times out it is not asked again for this long.
const KEYRING_BACKOFF: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeySource {
    Env,
    Keyring,
    File,
}

impl KeySource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Env => "environment",
            Self::Keyring => "OS keyring",
            Self::File => "key file",
        }
    }
}

/// The first key found in order: environment, then keyring, then file. Empty
/// values count as missing. The later sources are only asked when needed.
pub fn resolve_order(
    env: Option<String>,
    keyring: impl FnOnce() -> Option<String>,
    file: impl FnOnce() -> Option<String>,
) -> Option<(String, KeySource)> {
    let usable = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
    usable(env)
        .map(|key| (key, KeySource::Env))
        .or_else(|| usable(keyring()).map(|key| (key, KeySource::Keyring)))
        .or_else(|| usable(file()).map(|key| (key, KeySource::File)))
}

/// The key for `account` (`groq`), looked up in order. `env_var` names its
/// environment variable.
pub fn resolve(account: &str, env_var: &str) -> Option<(String, KeySource)> {
    resolve_order(
        std::env::var(env_var).ok(),
        || keyring_get(account),
        || {
            file_path(account)
                .ok()
                .and_then(|path| read_key_file(&path))
        },
    )
    .map(|(key, source)| (key.trim().to_string(), source))
}

/// [`resolve`], plus whether the keyring could not be asked (it failed, timed
/// out or is backing off) when nothing was found: then `None` may only mean
/// "unreadable right now".
pub fn resolve_checked(account: &str, env_var: &str) -> (Option<(String, KeySource)>, bool) {
    let found = resolve(account, env_var);
    let unreadable = found.is_none() && !keyring_usable();
    (found, unreadable)
}

/// Where a key resolves from, or `None` when there is none.
pub fn status(account: &str, env_var: &str) -> Option<KeySource> {
    resolve(account, env_var).map(|(_, source)| source)
}

/// Saves `key`: in the keyring, or in the 0600 key file when the keyring is
/// unavailable. Returns where it went.
pub fn store(account: &str, key: &str) -> Result<KeySource> {
    let key = key.trim();
    if key.is_empty() {
        return Err(eyre!("the key is empty"));
    }
    if keyring_set(account, key) {
        return Ok(KeySource::Keyring);
    }
    write_key_file(&file_path(account)?, key)?;
    Ok(KeySource::File)
}

/// `groq.key` beside `settings.json`.
pub fn file_path(account: &str) -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join(format!("{account}.key")))
}

pub(crate) fn write_key_file(path: &Path, key: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("key.tmp");
    let _ = fs::remove_file(&temp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temp)?;
    file.write_all(key.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    Ok(())
}

/// The key in `path`, unless the file is readable by anyone but its owner.
pub(crate) fn read_key_file(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).ok()?.permissions().mode();
        if mode & 0o077 != 0 {
            tracing::warn!(
                path = %path.display(),
                "ignoring a key file others can read; run `chmod 600` on it"
            );
            return None;
        }
    }
    let key = fs::read_to_string(path).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_string())
}

/// When the keyring last failed or timed out.
static KEYRING_FAILED: Mutex<Option<Instant>> = Mutex::new(None);

fn keyring_usable() -> bool {
    KEYRING_FAILED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .is_none_or(|failed| failed.elapsed() >= KEYRING_BACKOFF)
}

fn keyring_failed() {
    *KEYRING_FAILED
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
}

/// Runs `work` on its own thread and waits at most [`KEYRING_DEADLINE`].
fn bounded<T: Send + 'static>(work: impl FnOnce() -> Option<T> + Send + 'static) -> Option<T> {
    if !keyring_usable() {
        return None;
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("keyring".into())
        .spawn(move || {
            let _ = sender.send(work());
        })
        .ok()?;
    match receiver.recv_timeout(KEYRING_DEADLINE) {
        Ok(value) => value,
        Err(_) => {
            keyring_failed();
            None
        }
    }
}

fn keyring_get(account: &str) -> Option<String> {
    let account = account.to_string();
    bounded(move || {
        let entry = keyring::Entry::new(SERVICE, &account).ok()?;
        match entry.get_password() {
            Ok(key) => Some(key),
            Err(keyring::Error::NoEntry) => None,
            Err(error) => {
                tracing::debug!(%error, "the OS keyring is unavailable");
                keyring_failed();
                None
            }
        }
    })
}

fn keyring_set(account: &str, key: &str) -> bool {
    let (account, key) = (account.to_string(), key.to_string());
    let stored = bounded(move || {
        let entry = keyring::Entry::new(SERVICE, &account).ok()?;
        match entry.set_password(&key) {
            Ok(()) => Some(()),
            Err(error) => {
                tracing::debug!(%error, "the OS keyring could not store the key");
                None
            }
        }
    });
    if stored.is_none() {
        keyring_failed();
    }
    stored.is_some()
}

/// The providers that take a key, and the environment variable that
/// overrides each one's stored key. The gateway's "key" is its bearer token.
pub const PROVIDER_ENV: &[(&str, &str)] = &[
    ("groq", "GROQ_API_KEY"),
    ("openrouter", "OPENROUTER_API_KEY"),
    ("openai", "OPENAI_API_KEY"),
    ("deepgram", "DEEPGRAM_API_KEY"),
    ("gateway", "WORDINK_GATEWAY_TOKEN"),
];

/// The names `wordink keys set` takes.
pub const PROVIDERS: &[&str] = &["groq", "openrouter", "openai", "deepgram", "gateway"];

/// The environment variable for `provider`'s key.
pub fn env_var(provider: &str) -> Option<&'static str> {
    PROVIDER_ENV
        .iter()
        .find(|(name, _)| *name == provider)
        .map(|(_, env)| *env)
}

/// `wordink keys set <provider>`: reads the key from `input` (stdin, never
/// argv, so it stays out of shell history and the process list) and hands it
/// to `save`. Returns a line to print, which never contains the key.
pub fn set_from(
    provider: &str,
    input: &mut impl std::io::BufRead,
    save: impl FnOnce(&str, &str) -> Result<KeySource>,
) -> Result<String> {
    if !PROVIDERS.contains(&provider) {
        return Err(eyre!(
            "unknown provider `{provider}`; expected one of: {}",
            PROVIDERS.join(", ")
        ));
    }
    let mut line = String::new();
    input.read_line(&mut line)?;
    let key = line.trim();
    if key.is_empty() {
        return Err(eyre!(
            "no key on stdin; pipe it in, e.g. `omaseal get ... | wordink keys set {provider}`"
        ));
    }
    let source = save(provider, key)?;
    Ok(format!("{provider}: key saved ({})", source.label()))
}

/// `wordink keys status`: one `provider: set|unset` line each.
pub fn status_lines(is_set: impl Fn(&str) -> bool) -> Vec<String> {
    PROVIDERS
        .iter()
        .map(|provider| {
            format!(
                "{provider}: {}",
                if is_set(provider) { "set" } else { "unset" }
            )
        })
        .collect()
}

/// Runs `wordink keys set|status` for the command line. Exit code 0 or 1.
pub fn run_cli(command: &KeysCommand) -> i32 {
    match command {
        KeysCommand::Set { provider } => {
            let stdin = std::io::stdin();
            match set_from(provider, &mut stdin.lock(), store) {
                Ok(line) => {
                    println!("{line}");
                    0
                }
                Err(error) => {
                    eprintln!("wordink: {error:#}");
                    1
                }
            }
        }
        KeysCommand::Status => {
            for line in status_lines(|provider| {
                env_var(provider).is_some_and(|env| status(provider, env).is_some())
            }) {
                println!("{line}");
            }
            0
        }
    }
}

/// `wordink keys ...`.
#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum KeysCommand {
    /// Save a provider's API key, read from stdin (never from an argument).
    Set {
        /// `groq`, `openrouter`, `openai`, `deepgram` or `gateway`.
        provider: String,
    },
    /// Show whether each provider's key is set. Never prints a key.
    Status,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn the_environment_beats_the_keyring_and_the_keyring_beats_the_file() {
        let found = resolve_order(some("env-key"), || some("ring-key"), || some("file-key"));
        assert_eq!(found, Some(("env-key".into(), KeySource::Env)));
        let found = resolve_order(None, || some("ring-key"), || some("file-key"));
        assert_eq!(found, Some(("ring-key".into(), KeySource::Keyring)));
        let found = resolve_order(None, || None, || some("file-key"));
        assert_eq!(found, Some(("file-key".into(), KeySource::File)));
        assert_eq!(resolve_order(None, || None, || None), None);
    }

    #[test]
    fn an_empty_value_counts_as_missing_and_later_sources_are_skipped_once_found() {
        let found = resolve_order(some("  "), || some("ring-key"), || some("file-key"));
        assert_eq!(found, Some(("ring-key".into(), KeySource::Keyring)));
        let asked = Cell::new(false);
        let found = resolve_order(
            some("env-key"),
            || {
                asked.set(true);
                None
            },
            || {
                asked.set(true);
                None
            },
        );
        assert_eq!(found.map(|(_, source)| source), Some(KeySource::Env));
        assert!(
            !asked.get(),
            "the keyring is never asked when the environment answers"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_owner_only_and_a_wider_one_is_ignored() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("wordink-keys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join("groq.key");
        write_key_file(&path, "secret-value").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read_key_file(&path).as_deref(), Some("secret-value"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_key_file(&path), None);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn set_reads_the_key_from_the_input_trims_it_and_never_echoes_it() {
        let mut saved = None;
        let line = set_from(
            "groq",
            &mut std::io::Cursor::new("  gsk_secret123 \n"),
            |account, key| {
                saved = Some((account.to_string(), key.to_string()));
                Ok(KeySource::File)
            },
        )
        .unwrap();
        assert_eq!(saved, Some(("groq".into(), "gsk_secret123".into())));
        assert!(!line.contains("gsk_secret123"));
        assert!(line.contains("groq"));
    }

    #[test]
    fn set_refuses_an_empty_key_and_an_unknown_provider_without_saving() {
        let refuse = |_: &str, _: &str| -> Result<KeySource> { panic!("must not save") };
        assert!(set_from("groq", &mut std::io::Cursor::new("\n"), refuse).is_err());
        assert!(set_from("nope", &mut std::io::Cursor::new("k\n"), refuse).is_err());
    }

    #[test]
    fn status_shows_only_set_or_unset() {
        let lines = status_lines(|provider| provider == "groq");
        assert_eq!(lines[0], "groq: set");
        assert!(lines[1..].iter().all(|line| line.ends_with(": unset")));
        assert_eq!(lines.len(), PROVIDERS.len());
        assert!(
            status_lines(|_| false)
                .iter()
                .all(|line| line.ends_with(": unset"))
        );
    }

    #[test]
    fn every_provider_has_its_own_environment_variable_and_account() {
        use crate::transcription::cloud::Provider;
        let expected = [
            ("groq", "GROQ_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("deepgram", "DEEPGRAM_API_KEY"),
            ("gateway", "WORDINK_GATEWAY_TOKEN"),
        ];
        for (provider, env) in expected {
            assert_eq!(env_var(provider), Some(env));
            assert!(PROVIDERS.contains(&provider));
            let known = Provider::from_id(provider).expect("a cloud provider");
            assert_eq!(
                known.key_env(),
                env,
                "the engine reads the same variable the CLI reports"
            );
        }
        assert_eq!(PROVIDERS.len(), Provider::ALL.len());
        assert_eq!(env_var("nope"), None);
    }

    #[test]
    fn a_new_providers_key_resolves_env_then_keyring_then_file_like_groqs() {
        // The same order, for each account name.
        for provider in PROVIDERS {
            let found = resolve_order(None, || None, || some(&format!("{provider}-file-key")));
            assert_eq!(
                found,
                Some((format!("{provider}-file-key"), KeySource::File))
            );
            let found = resolve_order(some("env"), || some("ring"), || some("file"));
            assert_eq!(found, Some(("env".into(), KeySource::Env)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn each_provider_keeps_its_key_in_its_own_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("wordink-keys-providers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        for provider in PROVIDERS {
            let path = directory.join(format!("{provider}.key"));
            write_key_file(&path, &format!("{provider}-secret")).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(read_key_file(&path), Some(format!("{provider}-secret")));
        }
        assert!(file_path("openrouter").unwrap().ends_with("openrouter.key"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn set_accepts_every_provider_and_saves_to_its_account() {
        for provider in PROVIDERS {
            let mut saved = None;
            let line = set_from(
                provider,
                &mut std::io::Cursor::new("tok-123\n"),
                |account, key| {
                    saved = Some((account.to_string(), key.to_string()));
                    Ok(KeySource::Keyring)
                },
            )
            .unwrap();
            assert_eq!(saved, Some((provider.to_string(), "tok-123".to_string())));
            assert!(line.starts_with(provider) && !line.contains("tok-123"));
        }
    }
}
