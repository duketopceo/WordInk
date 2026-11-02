//! The cloud transcription engine family (KTD4), beside transcribe.cpp.
//!
//! One OpenAI-compatible `/audio/transcriptions` client over Hex's `net`
//! layer. Groq is the provider so far (`whisper-large-v3-turbo`). A dictation
//! whose engine resolves here sends its 16 kHz clip as a mono WAV, never loads
//! a local model, and records its usage and estimated cost (KTD5).

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::history::{HistoryRetention, UsageRecord};

const SAMPLE_RATE: u32 = 16_000;
/// How many kept clips stay on disk; the oldest go first.
const MAX_KEPT_CLIPS: usize = 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest WAV sent in one request. Groq's upload limit is 25 MB; this
/// leaves headroom for the multipart framing. At 16 kHz mono 16-bit that is a
/// little over 13 minutes.
const MAX_UPLOAD_BYTES: usize = 24 * 1024 * 1024;
/// Where failed-dictation clips live, beside `history.json`.
pub const FAILED_CLIPS_DIR: &str = "failed-dictations";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    Groq,
}

impl Provider {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Groq => "Groq",
        }
    }

    /// The wire name: the usage record's provider, the key's account.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Groq => "groq",
        }
    }

    pub const fn model(self) -> &'static str {
        match self {
            Self::Groq => "whisper-large-v3-turbo",
        }
    }

    pub const fn endpoint(self) -> &'static str {
        match self {
            Self::Groq => "https://api.groq.com/openai/v1/audio/transcriptions",
        }
    }

    pub const fn key_env(self) -> &'static str {
        match self {
            Self::Groq => "GROQ_API_KEY",
        }
    }

    /// Cloud engines take the whole clip; none of them stream.
    pub const fn streams(self) -> bool {
        false
    }
}

/// Which engine dictation uses. `Auto` is Groq when a Groq key resolves and
/// the local model otherwise.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineChoice {
    #[default]
    Auto,
    Local,
    Groq,
}

/// The cloud provider `choice` runs, given whether a Groq key resolves.
pub fn route_for(choice: EngineChoice, groq_key_resolves: bool) -> Option<Provider> {
    match choice {
        EngineChoice::Local => None,
        EngineChoice::Groq => Some(Provider::Groq),
        EngineChoice::Auto => groq_key_resolves.then_some(Provider::Groq),
    }
}

/// [`route_for`] for an engine that has no local model (`cloud_only`): unless
/// the person chose Local, it can only run on the cloud, so a key that
/// momentarily can't be read surfaces as "no Groq key" instead of falling to
/// a local path that does not exist.
pub fn route_for_engine(
    choice: EngineChoice,
    groq_key_resolves: bool,
    cloud_only: bool,
) -> Option<Provider> {
    if cloud_only && choice != EngineChoice::Local {
        return Some(Provider::Groq);
    }
    route_for(choice, groq_key_resolves)
}

static ENGINE_CHOICE: AtomicU8 = AtomicU8::new(0);

pub fn set_engine_choice(choice: EngineChoice) {
    ENGINE_CHOICE.store(choice as u8, Ordering::Release);
    // The key is resolved off the hotkey thread, here and when it goes stale.
    refresh_key_in_background();
}

fn engine_choice() -> EngineChoice {
    match ENGINE_CHOICE.load(Ordering::Acquire) {
        1 => EngineChoice::Local,
        2 => EngineChoice::Groq,
        _ => EngineChoice::Auto,
    }
}

/// The provider the current settings route dictation to, or `None` for the
/// local model. Reads only the in-memory key; never touches the keyring.
pub fn route() -> Option<Provider> {
    route_with(false)
}

/// [`route`] for an engine with no local model when `cloud_only`.
pub fn route_with(cloud_only: bool) -> Option<Provider> {
    #[cfg(test)]
    if let Some(forced) = ROUTE_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    route_for_engine(engine_choice(), cached_key().is_some(), cloud_only)
}

/// How long a resolved key is trusted before a background refresh picks up
/// a key saved by `wordink keys set` in another process.
const KEY_STALE_AFTER: Duration = Duration::from_secs(30);

/// What a key resolution found. `unreadable` means the keyring could not be
/// asked (it failed, timed out or is backing off), so `key: None` may not be
/// the truth.
pub struct Resolution {
    pub key: Option<String>,
    pub unreadable: bool,
}

#[derive(Default)]
struct CachedKey {
    resolved_at: Option<Instant>,
    key: Option<Arc<str>>,
}

/// The resolved provider key, held in memory so the hotkey path never waits
/// on the keyring. Refreshed off that path.
pub struct KeyCache {
    state: Mutex<CachedKey>,
    refreshing: AtomicBool,
}

impl KeyCache {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(CachedKey {
                resolved_at: None,
                key: None,
            }),
            refreshing: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CachedKey> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// The cached key and whether it is due a refresh. Never blocks on I/O.
    pub fn peek(&self, stale_after: Duration) -> (Option<Arc<str>>, bool) {
        let state = self.lock();
        let stale = state
            .resolved_at
            .is_none_or(|at| at.elapsed() >= stale_after);
        (state.key.clone(), stale)
    }

    /// Stores what `resolve` finds. A key that could not be read this time
    /// (keyring down) keeps the last one found instead of dropping it.
    pub fn refresh(&self, resolve: impl FnOnce() -> Resolution) {
        let found = resolve();
        let mut state = self.lock();
        match found.key {
            Some(key) => state.key = Some(Arc::from(key)),
            None if found.unreadable && state.key.is_some() => {}
            None => state.key = None,
        }
        state.resolved_at = Some(Instant::now());
    }

    fn begin_refresh(&self) -> bool {
        !self.refreshing.swap(true, Ordering::AcqRel)
    }

    fn end_refresh(&self) {
        self.refreshing.store(false, Ordering::Release);
    }
}

#[cfg(test)]
thread_local! {
    /// What this test thread's routes resolve to, in place of the settings.
    static ROUTE_OVERRIDE: std::cell::Cell<Option<Option<Provider>>> =
        const { std::cell::Cell::new(None) };
}

/// Forces [`route_with`] on this thread to `route`, for tests.
#[cfg(test)]
pub(crate) fn force_route(route: Option<Provider>) {
    ROUTE_OVERRIDE.with(|forced| forced.set(Some(route)));
}

static GROQ_KEY: KeyCache = KeyCache::new();

fn resolve_groq() -> Resolution {
    let (found, unreadable) = crate::keys::resolve_checked(Provider::Groq.id(), Provider::Groq.key_env());
    Resolution {
        key: found.map(|(key, _)| key),
        unreadable,
    }
}

/// Resolves the Groq key now. Blocks for as long as the keyring takes (up to
/// its deadline), so call it only from startup or a worker, never from the
/// hotkey thread. Unit tests never see the machine's real key, so a
/// developer's own `GROQ_API_KEY` can't change which engine a test runs.
pub fn refresh_key() {
    if cfg!(test) {
        return;
    }
    GROQ_KEY.refresh(resolve_groq);
}

/// [`refresh_key`] on its own thread, at most one at a time.
pub fn refresh_key_in_background() {
    if cfg!(test) || !GROQ_KEY.begin_refresh() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("groq-key".into())
        .spawn(|| {
            refresh_key();
            GROQ_KEY.end_refresh();
        });
    if spawned.is_err() {
        GROQ_KEY.end_refresh();
    }
}

/// The cached Groq key. Never blocks: a stale or missing entry schedules a
/// background refresh and answers with what is held.
fn cached_key() -> Option<Arc<str>> {
    let (key, stale) = GROQ_KEY.peek(KEY_STALE_AFTER);
    if stale {
        refresh_key_in_background();
    }
    key
}

/// The key for a request. Called on a transcription thread, never the hotkey
/// thread, so when nothing is cached it may resolve (and wait) here.
fn key_for_request() -> Option<Arc<str>> {
    if let Some(key) = cached_key() {
        return Some(key);
    }
    refresh_key();
    GROQ_KEY.peek(KEY_STALE_AFTER).0
}

/// Why a cloud transcription failed, in words for the pill.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CloudError {
    NoKey,
    KeyRejected,
    RateLimited,
    Unavailable,
    Unreachable,
    Rejected(u16),
    BadReply,
    TooLong,
}

impl CloudError {
    pub fn message(&self, provider: Provider) -> String {
        let name = provider.name();
        match self {
            Self::NoKey => format!("no {name} key set \u{2014} run `wordink keys set {}`", provider.id()),
            Self::KeyRejected => format!("{name} key rejected"),
            Self::RateLimited => format!("{name} rate limit \u{2014} try again shortly"),
            Self::Unavailable => format!("{name} is unavailable"),
            Self::Unreachable => format!("can't reach {name}"),
            Self::Rejected(status) => format!("{name} could not transcribe this clip (HTTP {status})"),
            Self::BadReply => format!("{name} sent an unreadable reply"),
            Self::TooLong => format!("Recording too long for {name} (about 13 min max)"),
        }
    }
}

/// The language to tell the provider: a two-letter code, or nothing (the
/// provider detects it) for `auto` and for codes Whisper's API doesn't take.
pub fn language_hint(language: &str) -> Option<String> {
    (language.len() == 2 && language.chars().all(|c| c.is_ascii_lowercase()))
        .then(|| language.to_string())
}

/// Size of the WAV [`wav_bytes`] makes for `samples`: a 44-byte header and
/// two bytes a sample.
fn wav_len(samples: usize) -> usize {
    44 + samples.saturating_mul(2)
}

/// Refuses a clip whose WAV would not fit one request, before anything is
/// built or sent.
fn check_size(samples: &[f32]) -> Result<(), CloudError> {
    if wav_len(samples.len()) > MAX_UPLOAD_BYTES {
        return Err(CloudError::TooLong);
    }
    Ok(())
}

/// Mono 16 kHz 16-bit WAV bytes.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Ok(mut writer) = hound::WavWriter::new(&mut bytes, spec) {
        for sample in samples {
            let _ = writer.write_sample((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
        }
        let _ = writer.finalize();
    }
    bytes.into_inner()
}

/// A `multipart/form-data` body with `model`, optional `language`, and the
/// WAV as `file`.
pub fn multipart_body(boundary: &str, model: &str, language: Option<&str>, wav: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 512);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    };
    field("model", model);
    field("response_format", "json");
    field("temperature", "0");
    if let Some(language) = language {
        field("language", language);
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"dictation.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

pub const MINIMUM_BILLED_SECONDS: f64 = 10.0;

/// One row of the bundled price table.
pub struct Price {
    pub provider: &'static str,
    pub model: &'static str,
    pub usd_per_hour: f64,
    /// Seconds billed at least, per request.
    pub minimum_seconds: f64,
}

/// Bundled prices. Groq whisper-large-v3-turbo is $0.04 per hour of audio,
/// billed at least 10 seconds per request.
pub const PRICES: &[Price] = &[Price {
    provider: "groq",
    model: "whisper-large-v3-turbo",
    usd_per_hour: 0.04,
    minimum_seconds: MINIMUM_BILLED_SECONDS,
}];

/// Estimated cost of one request of `audio_seconds`, or `None` for a model
/// the table doesn't list.
pub fn estimate_cost(provider: &str, model: &str, audio_seconds: f64) -> Option<f64> {
    let price = PRICES
        .iter()
        .find(|price| price.provider == provider && price.model == model)?;
    Some(audio_seconds.max(price.minimum_seconds) * price.usd_per_hour / 3600.0)
}

/// Transcribes `samples` at `endpoint` with `key`.
pub fn transcribe_at(
    endpoint: &str,
    key: &str,
    model: &str,
    language: Option<&str>,
    samples: &[f32],
) -> Result<String, CloudError> {
    check_size(samples)?;
    let boundary = format!("wordink-{:016x}", boundary_nonce());
    let body = multipart_body(&boundary, model, language, &wav_bytes(samples));
    let agent = crate::net::agent(endpoint, |config| {
        config.timeout_global(Some(REQUEST_TIMEOUT))
    });
    let mut response = agent
        .post(endpoint)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send(&body[..])
        .map_err(|error| match error {
            ureq::Error::StatusCode(401 | 403) => CloudError::KeyRejected,
            ureq::Error::StatusCode(429) => CloudError::RateLimited,
            ureq::Error::StatusCode(status) if status >= 500 => CloudError::Unavailable,
            ureq::Error::StatusCode(status) => CloudError::Rejected(status),
            // The URL is not in the message, and the key never was.
            other => {
                tracing::warn!(reason = %crate::net::reason(&other), "cloud transcription request failed");
                CloudError::Unreachable
            }
        })?;
    let reply: serde_json::Value = response
        .body_mut()
        .read_to_string()
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or(CloudError::BadReply)?;
    reply
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(|text| text.trim().to_string())
        .ok_or(CloudError::BadReply)
}

fn boundary_nonce() -> u64 {
    let mut bytes = [0_u8; 8];
    let _ = getrandom::fill(&mut bytes);
    u64::from_le_bytes(bytes)
}

/// Where a failed clip may be kept: the directory and the history retention
/// that bounds how long it stays.
#[derive(Clone, Copy)]
pub struct KeepClips<'a> {
    pub dir: &'a Path,
    pub retention: HistoryRetention,
}

/// Transcribes with `provider` at `endpoint` (tests point it at a mock), and
/// on failure keeps the clip under `keep` so history can offer a retry. A
/// clip too long to send is not kept, and neither is one when `keep` is
/// `None`.
pub fn run_at(
    provider: Provider,
    endpoint: &str,
    key: Option<&str>,
    samples: &[f32],
    language: Option<&str>,
    keep: Option<KeepClips<'_>>,
) -> Result<(String, UsageRecord), String> {
    let outcome = match (check_size(samples), key.filter(|key| !key.is_empty())) {
        (Err(error), _) => Err(error),
        (Ok(()), None) => Err(CloudError::NoKey),
        (Ok(()), Some(key)) => transcribe_at(endpoint, key, provider.model(), language, samples),
    };
    match outcome {
        Ok(text) => {
            let audio_seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
            let usage = UsageRecord {
                provider: provider.id().into(),
                model: provider.model().into(),
                audio_seconds,
                words: text.split_whitespace().count() as u32,
                cost_usd: estimate_cost(provider.id(), provider.model(), audio_seconds)
                    .unwrap_or(0.0),
            };
            Ok((text, usage))
        }
        Err(error) => {
            if error != CloudError::TooLong {
                match keep {
                    Some(keep) => match keep_clip(keep.dir, keep.retention, samples) {
                        Ok(Some(path)) => tracing::warn!(
                            clip = %path.display(),
                            "kept the clip of a failed {} transcription",
                            provider.name()
                        ),
                        Ok(None) => {}
                        Err(io) => {
                            tracing::warn!(%io, "could not keep the clip of a failed transcription");
                        }
                    },
                    None => tracing::warn!("no place to keep the clip of a failed transcription"),
                }
            }
            Err(error.message(provider))
        }
    }
}

/// The failed-clip directory beside `history.json`, or `None` when the
/// support directory can't be found. There is no fallback location.
fn failed_clips_dir() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|dir| dir.join(FAILED_CLIPS_DIR))
}

/// Transcribes `samples` with `provider` using the cached key, keeping a
/// failed clip in `failed-dictations` beside history. Runs on a
/// transcription thread.
pub fn run(
    provider: Provider,
    samples: &[f32],
    language: Option<&str>,
) -> Result<(String, UsageRecord), String> {
    let key = key_for_request();
    let dir = failed_clips_dir();
    let keep = dir.as_deref().map(|dir| KeepClips {
        dir,
        retention: clip_retention(),
    });
    run_at(provider, provider.endpoint(), key.as_deref(), samples, language, keep)
}

/// `wordink transcribe-file <wav>` (hidden dev command): runs a WAV through
/// the cloud engine and prints the text, then its usage on stderr. Spends
/// real money when a key resolves, so it is not part of any test.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn transcribe_file_cli(path: &Path) -> i32 {
    // A one-shot process: resolve the key before the first route() reads it.
    refresh_key();
    let Some(provider) = route() else {
        eprintln!("wordink: no cloud engine is set up; run `wordink keys set groq`");
        return 1;
    };
    let samples = match crate::transcription::local_transcriber::read_wav_for_parakeet(path) {
        Ok(samples) => samples,
        Err(error) => {
            eprintln!("wordink: {error:#}");
            return 1;
        }
    };
    match run(provider, &samples, None) {
        Ok((text, usage)) => {
            println!("{text}");
            eprintln!(
                "usage: provider={} model={} audio_seconds={:.2} words={} cost_usd={:.8}",
                usage.provider, usage.model, usage.audio_seconds, usage.words, usage.cost_usd
            );
            0
        }
        Err(message) => {
            eprintln!("wordink: {message}");
            1
        }
    }
}

static CLIP_RETENTION: AtomicU8 = AtomicU8::new(2);

/// Follows the history retention setting; sweeps clips that fall outside it.
pub fn set_clip_retention(retention: HistoryRetention) {
    let index = HistoryRetention::ALL
        .iter()
        .position(|candidate| *candidate == retention)
        .unwrap_or(2);
    CLIP_RETENTION.store(index as u8, Ordering::Release);
    sweep_failed_clips();
}

fn clip_retention() -> HistoryRetention {
    HistoryRetention::ALL
        .get(usize::from(CLIP_RETENTION.load(Ordering::Acquire)))
        .copied()
        .unwrap_or_default()
}

/// Deletes failed clips outside the retention window, on its own thread. Run
/// at startup and whenever the setting changes (unit tests never touch the
/// machine's real clips).
pub fn sweep_failed_clips() {
    if cfg!(test) {
        return;
    }
    let Some(dir) = failed_clips_dir() else {
        return;
    };
    let retention = clip_retention();
    let _ = std::thread::Builder::new()
        .name("clip-sweep".into())
        .spawn(move || sweep_clips(&dir, retention, SystemTime::now()));
}

/// Removes every `*.wav` in `dir` older than `retention` allows, then the
/// oldest beyond [`MAX_KEPT_CLIPS`]. Off and Forever set no age limit.
pub fn sweep_clips(dir: &Path, retention: HistoryRetention, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let max_age = retention.max_age_ms().map(Duration::from_millis);
    let mut clips = Vec::new();
    for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        let age = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or_default();
        if max_age.is_some_and(|max| age > max) {
            let _ = fs::remove_file(&path);
        } else {
            clips.push(path);
        }
    }
    clips.sort();
    for old in clips.iter().take(clips.len().saturating_sub(MAX_KEPT_CLIPS)) {
        let _ = fs::remove_file(old);
    }
}

/// Deletes every kept clip in `dir` ("Clear history").
pub fn clear_clips(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
        if path.extension().and_then(|e| e.to_str()) == Some("wav") {
            let _ = fs::remove_file(path);
        }
    }
}

/// Saves a clip as `failed-<ms>.wav` (mode 0600, in a 0700 directory), then
/// sweeps the directory. `Ok(None)` when history is off: no new clip is kept.
fn keep_clip(
    directory: &Path,
    retention: HistoryRetention,
    samples: &[f32],
) -> std::io::Result<Option<PathBuf>> {
    if retention.is_off() {
        return Ok(None);
    }
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = directory.join(format!("failed-{stamp}.wav"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&path)?.write_all(&wav_bytes(samples))?;
    sweep_clips(directory, retention, SystemTime::now());
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A one-request HTTP server: answers `status` with `body` and sends the
    /// request it received (headers and body) back to the test.
    fn mock(status: u16, body: &'static str) -> (String, mpsc::Receiver<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/openai/v1/audio/transcriptions", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                head.push_str(&line);
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            let _ = sender.send((head, request));
        });
        (url, receiver)
    }

    fn speech() -> Vec<f32> {
        (0..16_000 * 2)
            .map(|i| ((i as f32) * 0.05).sin() * 0.3)
            .collect()
    }

    fn keep(dir: &Path) -> Option<KeepClips<'_>> {
        Some(KeepClips {
            dir,
            retention: HistoryRetention::Week,
        })
    }

    fn temp(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("wordink-cloud-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn the_request_is_multipart_with_the_model_and_a_16khz_mono_wav_file() {
        let (url, received) = mock(200, r#"{"text":" hello world "}"#);
        let text = transcribe_at(&url, "k-123", "whisper-large-v3-turbo", None, &speech()).unwrap();
        assert_eq!(text, "hello world");
        let (head, body) = received.recv().unwrap();
        let head = head.to_ascii_lowercase();
        assert!(head.contains("authorization: bearer k-123"));
        assert!(head.contains("content-type: multipart/form-data; boundary="));
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("name=\"model\"\r\n\r\nwhisper-large-v3-turbo"));
        assert!(body_text.contains("name=\"file\"; filename=\"dictation.wav\""));
        let wav = body.windows(4).position(|window| window == b"RIFF").unwrap();
        let reader = hound::WavReader::new(Cursor::new(&body[wav..])).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        assert!(reader.duration() >= 32_000 - 1);
    }

    #[test]
    fn a_language_hint_is_sent_only_when_given() {
        let body = String::from_utf8(multipart_body("b", "m", Some("fr"), b"x")).unwrap();
        assert!(body.contains("name=\"language\"\r\n\r\nfr"));
        let body = String::from_utf8(multipart_body("b", "m", None, b"x")).unwrap();
        assert!(!body.contains("language"));
    }

    #[test]
    fn http_failures_read_as_the_pill_texts() {
        for (status, expected) in [
            (401, "Groq key rejected"),
            (429, "Groq rate limit \u{2014} try again shortly"),
            (500, "Groq is unavailable"),
            (503, "Groq is unavailable"),
        ] {
            let (url, _received) = mock(status, "{}");
            let error = transcribe_at(&url, "k", "m", None, &speech()).unwrap_err();
            assert_eq!(error.message(Provider::Groq), expected, "HTTP {status}");
        }
    }

    #[test]
    fn a_server_that_cannot_be_reached_reads_as_cant_reach() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/x");
        let error = transcribe_at(&url, "k", "m", None, &speech()).unwrap_err();
        assert_eq!(error.message(Provider::Groq), "can't reach Groq");
    }

    #[test]
    fn a_failed_clip_is_kept_owner_only_and_a_good_one_is_not() {
        let directory = temp("kept");
        let (url, _received) = mock(401, "{}");
        let error = run_at(Provider::Groq, &url, Some("k"), &speech(), None, keep(&directory)).unwrap_err();
        assert_eq!(error, "Groq key rejected");
        let kept: Vec<_> = fs::read_dir(&directory).unwrap().map(|entry| entry.unwrap().path()).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].extension().and_then(|e| e.to_str()), Some("wav"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&kept[0]).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(fs::metadata(&directory).unwrap().permissions().mode() & 0o777, 0o700);
        }
        hound::WavReader::open(&kept[0]).unwrap();

        let good = temp("not-kept");
        let (url, _received) = mock(200, r#"{"text":"fine"}"#);
        let (text, _) = run_at(Provider::Groq, &url, Some("k"), &speech(), None, keep(&good)).unwrap();
        assert_eq!(text, "fine");
        assert!(!good.exists() || fs::read_dir(&good).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&directory);
        let _ = fs::remove_dir_all(&good);
    }

    #[test]
    fn a_missing_key_fails_without_a_request_and_keeps_the_clip() {
        let directory = temp("nokey");
        let error = run_at(Provider::Groq, "http://127.0.0.1:1/x", None, &speech(), None, keep(&directory)).unwrap_err();
        assert_eq!(error, "no Groq key set \u{2014} run `wordink keys set groq`");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_default_engine_is_groq_only_when_a_key_resolves() {
        assert_eq!(route_for(EngineChoice::Auto, true), Some(Provider::Groq));
        assert_eq!(route_for(EngineChoice::Auto, false), None);
        assert_eq!(route_for(EngineChoice::Local, true), None);
        assert_eq!(route_for(EngineChoice::Groq, false), Some(Provider::Groq));
        assert_eq!(route_for(EngineChoice::Groq, true), Some(Provider::Groq));
    }

    #[test]
    fn the_engine_setting_has_stable_names_and_defaults_to_auto() {
        assert_eq!(EngineChoice::default(), EngineChoice::Auto);
        assert_eq!(serde_json::to_string(&EngineChoice::Groq).unwrap(), "\"groq\"");
        assert_eq!(serde_json::from_str::<EngineChoice>("\"local\"").unwrap(), EngineChoice::Local);
    }

    #[test]
    fn only_two_letter_codes_are_sent_as_the_language() {
        assert_eq!(language_hint("en").as_deref(), Some("en"));
        assert_eq!(language_hint("auto"), None);
        assert_eq!(language_hint("yue"), None);
    }

    #[test]
    fn cloud_engines_do_not_stream() {
        assert!(!Provider::Groq.streams());
    }

    #[test]
    fn cost_is_forty_cents_an_hour_with_a_ten_second_minimum() {
        let per_second = 0.04 / 3600.0;
        let cost = |seconds| estimate_cost("groq", "whisper-large-v3-turbo", seconds).unwrap();
        assert!((cost(60.0) - 60.0 * per_second).abs() < 1e-12);
        assert!((cost(3600.0) - 0.04).abs() < 1e-12);
        // Below ten seconds, ten are billed.
        assert!((cost(2.0) - 10.0 * per_second).abs() < 1e-12);
        assert!((cost(0.0) - 10.0 * per_second).abs() < 1e-12);
        assert!((cost(10.0) - 10.0 * per_second).abs() < 1e-12);
        assert_eq!(estimate_cost("groq", "no-such-model", 5.0), None);
    }

    #[test]
    fn a_success_records_provider_model_seconds_words_and_cost() {
        let directory = temp("usage");
        let (url, _received) = mock(200, r#"{"text":"hello there world"}"#);
        let (_, usage) = run_at(Provider::Groq, &url, Some("k"), &speech(), None, keep(&directory)).unwrap();
        assert_eq!(usage.provider, "groq");
        assert_eq!(usage.model, "whisper-large-v3-turbo");
        assert!((usage.audio_seconds - 2.0).abs() < 1e-9);
        assert_eq!(usage.words, 3);
        assert!((usage.cost_usd - 10.0 * 0.04 / 3600.0).abs() < 1e-12);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_key_that_cannot_be_read_right_now_keeps_the_last_one() {
        let cache = KeyCache::new();
        let (key, stale) = cache.peek(Duration::from_secs(30));
        assert!(key.is_none() && stale, "an empty cache is stale and answers without resolving");
        cache.refresh(|| Resolution { key: Some("k-1".into()), unreadable: false });
        // The keyring timed out or is backing off: the key stays.
        cache.refresh(|| Resolution { key: None, unreadable: true });
        assert_eq!(cache.peek(Duration::from_secs(30)).0.as_deref(), Some("k-1"));
        // A clean answer of "none" does clear it.
        cache.refresh(|| Resolution { key: None, unreadable: false });
        assert!(cache.peek(Duration::from_secs(30)).0.is_none());
        // Unreadable with nothing held stays empty, not stale forever.
        cache.refresh(|| Resolution { key: None, unreadable: true });
        assert!(!cache.peek(Duration::from_secs(30)).1);
    }

    #[test]
    fn a_model_less_engine_never_falls_back_to_local_while_the_key_is_unreadable() {
        for resolves in [true, false] {
            assert_eq!(route_for_engine(EngineChoice::Auto, resolves, true), Some(Provider::Groq));
            assert_eq!(route_for_engine(EngineChoice::Groq, resolves, true), Some(Provider::Groq));
            // Choosing Local still means no audio leaves the machine.
            assert_eq!(route_for_engine(EngineChoice::Local, resolves, true), None);
        }
        assert_eq!(route_for_engine(EngineChoice::Auto, false, false), None);
        assert_eq!(route_for_engine(EngineChoice::Auto, true, false), Some(Provider::Groq));
    }

    #[test]
    fn a_clip_over_the_upload_cap_is_refused_before_any_request_and_not_kept() {
        // 24 MiB exactly fits; one more sample does not.
        let fits = (MAX_UPLOAD_BYTES - 44) / 2;
        assert!(check_size(&vec![0.0; fits]).is_ok());
        let too_long = vec![0.0; fits + 1];
        assert_eq!(check_size(&too_long), Err(CloudError::TooLong));
        assert_eq!(
            CloudError::TooLong.message(Provider::Groq),
            "Recording too long for Groq (about 13 min max)"
        );
        for key in [Some("k"), None] {
            let directory = temp("toolong");
            // Port 1 refuses: a request would read as "can't reach Groq".
            let error = run_at(Provider::Groq, "http://127.0.0.1:1/x", key, &too_long, None, keep(&directory)).unwrap_err();
            assert_eq!(error, "Recording too long for Groq (about 13 min max)");
            assert!(!directory.exists(), "a too-long clip is not kept");
        }
    }

    #[test]
    fn without_a_support_dir_a_failed_clip_is_not_kept_anywhere() {
        let error = run_at(Provider::Groq, "http://127.0.0.1:1/x", None, &speech(), None, None).unwrap_err();
        assert!(error.starts_with("no Groq key set"));
        assert!(!std::env::temp_dir().join("wordink-failed-dictations").exists());
    }

    fn aged_clip(directory: &Path, name: &str, age: Duration) -> PathBuf {
        fs::create_dir_all(directory).unwrap();
        let path = directory.join(name);
        fs::write(&path, b"RIFF").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - age)
            .unwrap();
        path
    }

    #[test]
    fn the_sweep_drops_clips_older_than_the_retention_and_keeps_twenty() {
        let day = Duration::from_secs(24 * 3600);
        let directory = temp("sweep");
        let old = aged_clip(&directory, "failed-1.wav", 8 * day);
        let recent = aged_clip(&directory, "failed-2.wav", 6 * day);
        sweep_clips(&directory, HistoryRetention::Week, SystemTime::now());
        assert!(!old.exists() && recent.exists());
        // A shorter window takes the six-day-old one too.
        sweep_clips(&directory, HistoryRetention::Day, SystemTime::now());
        assert!(!recent.exists());
        // Forever has no age limit, but never more than twenty remain.
        for index in 0..25 {
            aged_clip(&directory, &format!("failed-{:03}.wav", 100 + index), Duration::from_secs(60));
        }
        sweep_clips(&directory, HistoryRetention::Forever, SystemTime::now());
        let left: Vec<_> = fs::read_dir(&directory).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left.len(), MAX_KEPT_CLIPS);
        assert!(directory.join("failed-124.wav").exists(), "the newest stay");
        assert!(!directory.join("failed-100.wav").exists(), "the oldest go");
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn every_write_sweeps_and_history_off_keeps_no_new_clip() {
        let day = Duration::from_secs(24 * 3600);
        let directory = temp("write");
        let stale = aged_clip(&directory, "failed-1.wav", 9 * day);
        let kept = keep_clip(&directory, HistoryRetention::Week, &speech()).unwrap().unwrap();
        assert!(kept.exists() && !stale.exists(), "the write swept the expired clip");
        let off = temp("off");
        assert!(keep_clip(&off, HistoryRetention::Off, &speech()).unwrap().is_none());
        assert!(!off.exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn clearing_history_clears_the_clips() {
        let directory = temp("clear");
        aged_clip(&directory, "failed-1.wav", Duration::from_secs(5));
        aged_clip(&directory, "failed-2.wav", Duration::from_secs(5));
        clear_clips(&directory);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&directory);
    }
}
