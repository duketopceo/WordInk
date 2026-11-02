//! The cloud transcription engine family (KTD4), beside transcribe.cpp.
//!
//! One OpenAI-compatible `/audio/transcriptions` client over Hex's `net`
//! layer. Groq is the provider so far (`whisper-large-v3-turbo`). A dictation
//! whose engine resolves here sends its 16 kHz clip as a mono WAV, never loads
//! a local model, and records its usage and estimated cost (KTD5).

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::history::UsageRecord;

const SAMPLE_RATE: u32 = 16_000;
/// How many kept clips stay on disk; the oldest go first.
const MAX_KEPT_CLIPS: usize = 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

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

static ENGINE_CHOICE: AtomicU8 = AtomicU8::new(0);

pub fn set_engine_choice(choice: EngineChoice) {
    ENGINE_CHOICE.store(choice as u8, Ordering::Release);
}

fn engine_choice() -> EngineChoice {
    match ENGINE_CHOICE.load(Ordering::Acquire) {
        1 => EngineChoice::Local,
        2 => EngineChoice::Groq,
        _ => EngineChoice::Auto,
    }
}

/// The provider the current settings route dictation to, or `None` for the
/// local model.
pub fn route() -> Option<Provider> {
    let choice = engine_choice();
    if choice == EngineChoice::Local {
        return None;
    }
    route_for(choice, groq_key().is_some())
}

/// Unit tests never see the machine's real key, so a developer's own
/// `GROQ_API_KEY` can't change which engine a test runs.
#[cfg(test)]
fn groq_key() -> Option<String> {
    None
}

#[cfg(not(test))]
fn groq_key() -> Option<String> {
    crate::keys::resolve(Provider::Groq.id(), Provider::Groq.key_env()).map(|(key, _)| key)
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
        }
    }
}

/// The language to tell the provider: a two-letter code, or nothing (the
/// provider detects it) for `auto` and for codes Whisper's API doesn't take.
pub fn language_hint(language: &str) -> Option<String> {
    (language.len() == 2 && language.chars().all(|c| c.is_ascii_lowercase()))
        .then(|| language.to_string())
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

/// Transcribes with `provider` at `endpoint` (tests point it at a mock), and
/// on failure keeps the clip in `keep_dir` so history can offer a retry.
pub fn run_at(
    provider: Provider,
    endpoint: &str,
    key: Option<&str>,
    samples: &[f32],
    language: Option<&str>,
    keep_dir: &Path,
) -> Result<(String, UsageRecord), String> {
    let outcome = match key.filter(|key| !key.is_empty()) {
        None => Err(CloudError::NoKey),
        Some(key) => transcribe_at(endpoint, key, provider.model(), language, samples),
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
            match keep_clip(keep_dir, samples) {
                Ok(path) => tracing::warn!(
                    clip = %path.display(),
                    "kept the clip of a failed {} transcription",
                    provider.name()
                ),
                Err(io) => tracing::warn!(%io, "could not keep the clip of a failed transcription"),
            }
            Err(error.message(provider))
        }
    }
}

/// Transcribes `samples` with `provider` using the resolved key, keeping a
/// failed clip in `failed-dictations` beside history.
pub fn run(
    provider: Provider,
    samples: &[f32],
    language: Option<&str>,
) -> Result<(String, UsageRecord), String> {
    let key = crate::keys::resolve(provider.id(), provider.key_env()).map(|(key, _)| key);
    let keep_dir = crate::app_paths::support_dir()
        .map(|dir| dir.join("failed-dictations"))
        .unwrap_or_else(|_| std::env::temp_dir().join("wordink-failed-dictations"));
    run_at(provider, provider.endpoint(), key.as_deref(), samples, language, &keep_dir)
}

/// `wordink transcribe-file <wav>` (hidden dev command): runs a WAV through
/// the cloud engine and prints the text, then its usage on stderr. Spends
/// real money when a key resolves, so it is not part of any test.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn transcribe_file_cli(path: &Path) -> i32 {
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

/// Saves a clip as `failed-<ms>.wav` (mode 0600, in a 0700 directory) and
/// drops the oldest beyond [`MAX_KEPT_CLIPS`].
fn keep_clip(directory: &Path, samples: &[f32]) -> std::io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = directory.join(format!("failed-{stamp}.wav"));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&path)?.write_all(&wav_bytes(samples))?;
    let mut clips: Vec<_> = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("wav"))
        .collect();
    clips.sort();
    for old in clips.iter().take(clips.len().saturating_sub(MAX_KEPT_CLIPS)) {
        let _ = fs::remove_file(old);
    }
    Ok(path)
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
        let error = run_at(Provider::Groq, &url, Some("k"), &speech(), None, &directory).unwrap_err();
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
        let (text, _) = run_at(Provider::Groq, &url, Some("k"), &speech(), None, &good).unwrap();
        assert_eq!(text, "fine");
        assert!(!good.exists() || fs::read_dir(&good).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&directory);
        let _ = fs::remove_dir_all(&good);
    }

    #[test]
    fn a_missing_key_fails_without_a_request_and_keeps_the_clip() {
        let directory = temp("nokey");
        let error = run_at(Provider::Groq, "http://127.0.0.1:1/x", None, &speech(), None, &directory).unwrap_err();
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
        let (_, usage) = run_at(Provider::Groq, &url, Some("k"), &speech(), None, &directory).unwrap();
        assert_eq!(usage.provider, "groq");
        assert_eq!(usage.model, "whisper-large-v3-turbo");
        assert!((usage.audio_seconds - 2.0).abs() < 1e-9);
        assert_eq!(usage.words, 3);
        assert!((usage.cost_usd - 10.0 * 0.04 / 3600.0).abs() < 1e-12);
        let _ = fs::remove_dir_all(&directory);
    }
}
