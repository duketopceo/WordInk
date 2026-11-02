//! The cloud transcription engine family (KTD4), beside transcribe.cpp.
//!
//! One OpenAI-compatible `/audio/transcriptions` client over Hex's `net`
//! layer, for Groq (`whisper-large-v3-turbo`, the default), OpenAI, and the
//! WordInk gateway; Deepgram's `/v1/listen`; and OpenRouter's audio models
//! through chat completions. A dictation whose engine resolves here sends
//! its 16 kHz clip as a mono WAV, never loads a local model, and records its
//! usage and estimated cost (KTD5).

pub mod gateway;
pub mod openrouter;

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
    OpenRouter,
    OpenAi,
    Deepgram,
    Gateway,
}

impl Provider {
    pub const ALL: [Self; 5] = [
        Self::Groq,
        Self::OpenRouter,
        Self::OpenAi,
        Self::Deepgram,
        Self::Gateway,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Groq => "Groq",
            Self::OpenRouter => "OpenRouter",
            Self::OpenAi => "OpenAI",
            Self::Deepgram => "Deepgram",
            Self::Gateway => "WordInk gateway",
        }
    }

    /// The wire name: the usage record's provider, the key's account.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::OpenRouter => "openrouter",
            Self::OpenAi => "openai",
            Self::Deepgram => "deepgram",
            Self::Gateway => "gateway",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|provider| provider.id() == id)
    }

    /// The model used until another is chosen.
    pub const fn default_model(self) -> &'static str {
        match self {
            Self::Groq => "whisper-large-v3-turbo",
            Self::OpenRouter => openrouter::DEFAULT_MODEL,
            Self::OpenAi => "gpt-4o-mini-transcribe",
            Self::Deepgram => "nova-3",
            // The gateway calls its own model and ignores this.
            Self::Gateway => "gateway",
        }
    }

    /// The fixed models of a provider with a short list; OpenRouter's come
    /// from its catalogue.
    pub const fn fixed_models(self) -> &'static [&'static str] {
        match self {
            Self::Groq => &["whisper-large-v3-turbo"],
            Self::OpenAi => &["gpt-4o-mini-transcribe", "whisper-1"],
            Self::Deepgram => &["nova-3"],
            Self::Gateway => &["gateway"],
            Self::OpenRouter => &[],
        }
    }

    /// Whether `model` can be sent to this provider. OpenRouter takes any
    /// `vendor/model` id, since its list changes daily.
    pub fn accepts_model(self, model: &str) -> bool {
        match self {
            Self::OpenRouter => model.contains('/') && !model.contains(char::is_whitespace),
            _ => self.fixed_models().contains(&model),
        }
    }

    /// The fixed endpoint, or `None` for the gateway, which has the
    /// person's own URL.
    pub const fn endpoint(self) -> Option<&'static str> {
        match self {
            Self::Groq => Some("https://api.groq.com/openai/v1/audio/transcriptions"),
            Self::OpenRouter => Some(openrouter::CHAT_URL),
            Self::OpenAi => Some("https://api.openai.com/v1/audio/transcriptions"),
            Self::Deepgram => Some("https://api.deepgram.com/v1/listen"),
            Self::Gateway => None,
        }
    }

    pub fn key_env(self) -> &'static str {
        crate::keys::env_var(self.id()).unwrap_or("")
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
    Openrouter,
    Openai,
    Deepgram,
    Gateway,
}

impl EngineChoice {
    pub const ALL: [Self; 7] = [
        Self::Auto,
        Self::Local,
        Self::Groq,
        Self::Openrouter,
        Self::Openai,
        Self::Deepgram,
        Self::Gateway,
    ];

    /// The provider this choice names, if it names one.
    pub const fn provider(self) -> Option<Provider> {
        match self {
            Self::Auto | Self::Local => None,
            Self::Groq => Some(Provider::Groq),
            Self::Openrouter => Some(Provider::OpenRouter),
            Self::Openai => Some(Provider::OpenAi),
            Self::Deepgram => Some(Provider::Deepgram),
            Self::Gateway => Some(Provider::Gateway),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Groq when a key is set)",
            Self::Local => "Local model",
            Self::Groq => "Groq",
            Self::Openrouter => "OpenRouter",
            Self::Openai => "OpenAI",
            Self::Deepgram => "Deepgram",
            Self::Gateway => "WordInk gateway",
        }
    }
}

/// The cloud provider `choice` runs, given whether a Groq key resolves.
pub fn route_for(choice: EngineChoice, groq_key_resolves: bool) -> Option<Provider> {
    match choice {
        EngineChoice::Local => None,
        EngineChoice::Auto => groq_key_resolves.then_some(Provider::Groq),
        named => named.provider(),
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
        return Some(choice.provider().unwrap_or(Provider::Groq));
    }
    route_for(choice, groq_key_resolves)
}

static ENGINE_CHOICE: AtomicU8 = AtomicU8::new(0);

/// The settings that pick a model and the gateway, beside the engine.
#[derive(Default)]
struct Chosen {
    model: Option<String>,
    gateway_url: Option<String>,
}

static CHOSEN: Mutex<Chosen> = Mutex::new(Chosen {
    model: None,
    gateway_url: None,
});

fn chosen() -> std::sync::MutexGuard<'static, Chosen> {
    CHOSEN.lock().unwrap_or_else(|error| error.into_inner())
}

/// Applies the engine settings: the choice, the model id (`engine_model`)
/// and the gateway URL (`gateway_url`).
pub fn set_engine(choice: EngineChoice, model: Option<&str>, gateway_url: Option<&str>) {
    ENGINE_CHOICE.store(choice as u8, Ordering::Release);
    {
        let mut chosen = chosen();
        chosen.model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_owned);
        chosen.gateway_url = gateway_url
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_owned);
    }
    // Keys are resolved off the hotkey thread, here and when they go stale.
    refresh_key_in_background();
    if choice == EngineChoice::Openrouter {
        refresh_catalogue_in_background();
    }
}

fn engine_choice() -> EngineChoice {
    EngineChoice::ALL
        .get(usize::from(ENGINE_CHOICE.load(Ordering::Acquire)))
        .copied()
        .unwrap_or_default()
}

/// The provider the engine setting names, if it names one.
pub fn chosen_provider() -> Option<Provider> {
    engine_choice().provider()
}

/// The model `provider` is sent: the chosen one when this provider accepts
/// it (a model left over from another provider is ignored), else its
/// default.
pub fn model_for(provider: Provider, chosen_model: Option<&str>) -> String {
    chosen_model
        .filter(|model| provider.accepts_model(model))
        .unwrap_or(provider.default_model())
        .to_owned()
}

/// The model the current settings send `provider`.
pub fn selected_model(provider: Provider) -> String {
    let chosen = chosen();
    model_for(provider, chosen.model.as_deref())
}

/// The URL and model dictation to the gateway uses, or why it can't.
fn endpoint_for(provider: Provider, gateway_url: Option<&str>) -> Result<String, CloudError> {
    match provider.endpoint() {
        Some(endpoint) => Ok(endpoint.to_owned()),
        None => gateway::endpoint(gateway_url.unwrap_or("")),
    }
}

/// The models to offer for `provider`: its fixed list, or OpenRouter's
/// audio-input catalogue (cache or bundled; never the network).
pub fn models_for(provider: Provider) -> Vec<(String, String)> {
    match provider {
        Provider::OpenRouter => openrouter_models()
            .into_iter()
            .map(|model| (model.id, model.name))
            .collect(),
        other => other
            .fixed_models()
            .iter()
            .map(|model| ((*model).to_owned(), (*model).to_owned()))
            .collect(),
    }
}

fn catalogue_path() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|dir| dir.join(openrouter::CACHE_FILE))
}

/// The catalogue as read from the cache file, held a minute so drawing the
/// model list never reads the file again.
static CATALOGUE_READ: Mutex<Option<(Instant, Vec<openrouter::Model>)>> = Mutex::new(None);

fn openrouter_models() -> Vec<openrouter::Model> {
    let Some(path) = catalogue_path().filter(|_| !cfg!(test)) else {
        return openrouter::bundled();
    };
    let mut held = CATALOGUE_READ
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    match &*held {
        Some((at, models)) if at.elapsed() < Duration::from_secs(60) => models.clone(),
        _ => {
            let models = openrouter::cached_or_bundled(&path);
            *held = Some((Instant::now(), models.clone()));
            models
        }
    }
}

/// Reads the catalogue (fetching it when the cache is a day old) on its own
/// thread, so the first OpenRouter dictation already has its prices.
pub fn refresh_catalogue_in_background() {
    if cfg!(test) {
        return;
    }
    let Some(path) = catalogue_path() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("openrouter-catalogue".into())
        .spawn(move || {
            let (_, source) = openrouter::load(&path, crate::history::now_ms(), || {
                openrouter::fetch(openrouter::MODELS_URL)
            });
            tracing::debug!(?source, "OpenRouter catalogue loaded");
        });
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
    route_for_engine(
        engine_choice(),
        cached_key(Provider::Groq).is_some(),
        cloud_only,
    )
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

static KEYS: [KeyCache; 5] = [
    KeyCache::new(),
    KeyCache::new(),
    KeyCache::new(),
    KeyCache::new(),
    KeyCache::new(),
];

fn key_cache(provider: Provider) -> &'static KeyCache {
    &KEYS[Provider::ALL
        .iter()
        .position(|candidate| *candidate == provider)
        .unwrap_or(0)]
}

fn resolve_key(provider: Provider) -> Resolution {
    let (found, unreadable) = crate::keys::resolve_checked(provider.id(), provider.key_env());
    Resolution {
        key: found.map(|(key, _)| key),
        unreadable,
    }
}

/// The providers whose keys are kept warm: Groq (the Auto default) and the
/// one the engine setting names.
fn warm_providers() -> Vec<Provider> {
    let mut providers = vec![Provider::Groq];
    if let Some(chosen) = engine_choice().provider()
        && chosen != Provider::Groq
    {
        providers.push(chosen);
    }
    providers
}

/// Resolves the warm keys now. Blocks for as long as the keyring takes (up to
/// its deadline), so call it only from startup or a worker, never from the
/// hotkey thread. Unit tests never see the machine's real keys, so a
/// developer's own `GROQ_API_KEY` can't change which engine a test runs.
pub fn refresh_key() {
    if cfg!(test) {
        return;
    }
    for provider in warm_providers() {
        key_cache(provider).refresh(|| resolve_key(provider));
    }
}

/// [`refresh_key`] on its own thread, at most one at a time.
pub fn refresh_key_in_background() {
    // One flag guards the whole refresh, whichever providers it covers.
    let guard = key_cache(Provider::Groq);
    if cfg!(test) || !guard.begin_refresh() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("cloud-keys".into())
        .spawn(|| {
            refresh_key();
            key_cache(Provider::Groq).end_refresh();
        });
    if spawned.is_err() {
        guard.end_refresh();
    }
}

/// The cached key. Never blocks: a stale or missing entry schedules a
/// background refresh and answers with what is held.
fn cached_key(provider: Provider) -> Option<Arc<str>> {
    let (key, stale) = key_cache(provider).peek(KEY_STALE_AFTER);
    if stale {
        refresh_key_in_background();
    }
    key
}

/// The key for a request. Called on a transcription thread, never the hotkey
/// thread, so when nothing is cached it may resolve (and wait) here.
fn key_for_request(provider: Provider) -> Option<Arc<str>> {
    if let Some(key) = cached_key(provider) {
        return Some(key);
    }
    if !cfg!(test) {
        key_cache(provider).refresh(|| resolve_key(provider));
    }
    key_cache(provider).peek(KEY_STALE_AFTER).0
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
    OutOfCredit,
    NoGateway,
    BadGatewayUrl,
    GatewayNotSecure,
}

impl CloudError {
    pub fn message(&self, provider: Provider) -> String {
        let name = provider.name();
        match self {
            Self::NoKey if provider == Provider::Gateway => {
                "no gateway token set \u{2014} run `wordink keys set gateway`".to_string()
            }
            Self::NoKey => format!(
                "no {name} key set \u{2014} run `wordink keys set {}`",
                provider.id()
            ),
            Self::KeyRejected => format!("{name} key rejected"),
            Self::RateLimited => format!("{name} rate limit \u{2014} try again shortly"),
            Self::Unavailable => format!("{name} is unavailable"),
            Self::Unreachable => format!("can't reach {name}"),
            Self::Rejected(status) => {
                format!("{name} could not transcribe this clip (HTTP {status})")
            }
            Self::BadReply => format!("{name} sent an unreadable reply"),
            Self::TooLong => format!("Recording too long for {name} (about 13 min max)"),
            Self::OutOfCredit => format!("{name} account is out of credit"),
            Self::NoGateway => {
                "no gateway URL set \u{2014} add `gateway_url` to settings.json".to_string()
            }
            Self::BadGatewayUrl => "the gateway URL is not valid".to_string(),
            Self::GatewayNotSecure => {
                "the gateway URL must be https (http only for localhost or 127.0.0.1)".to_string()
            }
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
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
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

/// Bundled prices, in US dollars per hour of audio. OpenRouter models are
/// priced from the catalogue instead.
///
/// Sources, checked 2026-11-02:
/// - Groq whisper-large-v3-turbo: $0.04/hour, 10 s minimum per request
///   (groq.com/pricing).
/// - OpenAI gpt-4o-mini-transcribe: $0.003/minute and whisper-1: $0.006/minute
///   (developers.openai.com/api/docs/pricing; OpenAI calls the first an
///   estimate, since the model bills by token).
/// - Deepgram nova-3 pre-recorded: $0.0043/minute, billed per second
///   (deepgram.com/pricing).
pub const PRICES: &[Price] = &[
    Price {
        provider: "groq",
        model: "whisper-large-v3-turbo",
        usd_per_hour: 0.04,
        minimum_seconds: MINIMUM_BILLED_SECONDS,
    },
    Price {
        provider: "openai",
        model: "gpt-4o-mini-transcribe",
        usd_per_hour: 0.003 * 60.0,
        minimum_seconds: 0.0,
    },
    Price {
        provider: "openai",
        model: "whisper-1",
        usd_per_hour: 0.006 * 60.0,
        minimum_seconds: 0.0,
    },
    Price {
        provider: "deepgram",
        model: "nova-3",
        usd_per_hour: 0.0043 * 60.0,
        minimum_seconds: 0.0,
    },
];

/// Estimated cost of one request of `audio_seconds`, or `None` for a model
/// the table doesn't list.
pub fn estimate_cost(provider: &str, model: &str, audio_seconds: f64) -> Option<f64> {
    let price = PRICES
        .iter()
        .find(|price| price.provider == provider && price.model == model)?;
    Some(audio_seconds.max(price.minimum_seconds) * price.usd_per_hour / 3600.0)
}

/// What a provider's HTTP failure means.
fn map_error(error: ureq::Error) -> CloudError {
    match error {
        ureq::Error::StatusCode(401 | 403) => CloudError::KeyRejected,
        ureq::Error::StatusCode(402) => CloudError::OutOfCredit,
        ureq::Error::StatusCode(429) => CloudError::RateLimited,
        ureq::Error::StatusCode(status) if status >= 500 => CloudError::Unavailable,
        ureq::Error::StatusCode(status) => CloudError::Rejected(status),
        // The URL is not in the message, and the key never was.
        other => {
            tracing::warn!(reason = %crate::net::reason(&other), "cloud transcription request failed");
            CloudError::Unreachable
        }
    }
}

fn read_json(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<serde_json::Value, CloudError> {
    response
        .body_mut()
        .read_to_string()
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or(CloudError::BadReply)
}

fn agent_for(endpoint: &str) -> ureq::Agent {
    crate::net::agent(endpoint, |config| {
        config.timeout_global(Some(REQUEST_TIMEOUT))
    })
}

/// Transcribes `samples` at an OpenAI-style `/audio/transcriptions`
/// `endpoint` (Groq, OpenAI, the gateway) with `key`.
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
    let response = agent_for(endpoint)
        .post(endpoint)
        .header("Authorization", &format!("Bearer {key}"))
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send(&body[..])
        .map_err(map_error)?;
    read_json(response)?
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(|text| text.trim().to_string())
        .ok_or(CloudError::BadReply)
}

/// The Deepgram `/v1/listen` URL for `model`: smart formatting, and the
/// language when there is a hint, else detection.
pub fn deepgram_url(endpoint: &str, model: &str, language: Option<&str>) -> String {
    let mut url = format!("{endpoint}?model={model}&smart_format=true");
    match language {
        Some(language) => {
            url.push_str("&language=");
            url.push_str(language);
        }
        None => url.push_str("&detect_language=true"),
    }
    url
}

/// Transcribes at Deepgram: the WAV as the raw body, `Authorization: Token`.
pub fn transcribe_deepgram(
    endpoint: &str,
    key: &str,
    model: &str,
    language: Option<&str>,
    samples: &[f32],
) -> Result<String, CloudError> {
    check_size(samples)?;
    let url = deepgram_url(endpoint, model, language);
    let response = agent_for(&url)
        .post(&url)
        .header("Authorization", &format!("Token {key}"))
        .header("Content-Type", "audio/wav")
        .send(&wav_bytes(samples)[..])
        .map_err(map_error)?;
    read_json(response)?
        .pointer("/results/channels/0/alternatives/0/transcript")
        .and_then(serde_json::Value::as_str)
        .map(|text| text.trim().to_string())
        .ok_or(CloudError::BadReply)
}

/// Transcribes with an OpenRouter audio model through chat completions.
/// Returns the text and the tokens the reply reports.
pub fn transcribe_openrouter(
    endpoint: &str,
    key: &str,
    model: &str,
    samples: &[f32],
) -> Result<(String, openrouter::Tokens), CloudError> {
    check_size(samples)?;
    let body = openrouter::chat_body(model, &wav_bytes(samples)).to_string();
    let response = agent_for(endpoint)
        .post(endpoint)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map_err(map_error)?;
    openrouter::parse_reply(&read_json(response)?).ok_or(CloudError::BadReply)
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

/// One transcription to make: where, with what, and what to price it by.
pub struct Call<'a> {
    pub provider: Provider,
    pub endpoint: &'a str,
    pub model: &'a str,
    pub key: Option<&'a str>,
    pub language: Option<&'a str>,
    /// The model's catalogue prices, for OpenRouter.
    pub catalogue: Option<&'a openrouter::Model>,
}

impl<'a> Call<'a> {
    pub fn new(provider: Provider, endpoint: &'a str, model: &'a str) -> Self {
        Self {
            provider,
            endpoint,
            model,
            key: None,
            language: None,
            catalogue: None,
        }
    }
}

/// Makes `call` and returns the text and its cost (`None` when the model
/// has no listed price).
fn send(call: &Call<'_>, samples: &[f32]) -> Result<(String, Option<f64>), CloudError> {
    check_size(samples)?;
    let key = call
        .key
        .filter(|key| !key.is_empty())
        .ok_or(CloudError::NoKey)?;
    let audio_seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
    let priced = |text| {
        (
            text,
            estimate_cost(call.provider.id(), call.model, audio_seconds),
        )
    };
    match call.provider {
        Provider::Groq | Provider::OpenAi | Provider::Gateway => {
            transcribe_at(call.endpoint, key, call.model, call.language, samples).map(priced)
        }
        Provider::Deepgram => {
            transcribe_deepgram(call.endpoint, key, call.model, call.language, samples).map(priced)
        }
        Provider::OpenRouter => {
            transcribe_openrouter(call.endpoint, key, call.model, samples).map(|(text, tokens)| {
                (
                    text,
                    call.catalogue.map(|model| openrouter::cost(model, tokens)),
                )
            })
        }
    }
}

/// Transcribes with `call` (tests point its endpoint at a mock), and on
/// failure keeps the clip under `keep` so history can offer a retry. A clip
/// too long to send is not kept, and neither is one when `keep` is `None`.
pub fn run_at(
    call: &Call<'_>,
    samples: &[f32],
    keep: Option<KeepClips<'_>>,
) -> Result<(String, UsageRecord), String> {
    let provider = call.provider;
    match send(call, samples) {
        Ok((text, cost)) => {
            let audio_seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
            let usage = UsageRecord {
                provider: provider.id().into(),
                model: call.model.into(),
                audio_seconds,
                words: text.split_whitespace().count() as u32,
                cost_usd: cost.unwrap_or(0.0),
            };
            Ok((text, usage))
        }
        Err(error) => {
            if error != CloudError::TooLong {
                match keep {
                    Some(keep) => match keep_clip(keep.dir, keep.retention, provider.id(), samples)
                    {
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
pub fn failed_clips_dir() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|dir| dir.join(FAILED_CLIPS_DIR))
}

/// Transcribes `samples` with `provider` using the cached key and the
/// selected model, keeping a failed clip in `failed-dictations` beside
/// history. Runs on a transcription thread.
pub fn run(
    provider: Provider,
    samples: &[f32],
    language: Option<&str>,
) -> Result<(String, UsageRecord), String> {
    let gateway_url = chosen().gateway_url.clone();
    run_with(
        provider,
        &selected_model(provider),
        gateway_url.as_deref(),
        samples,
        language,
        true,
    )
}

/// [`run`] with an explicit model and gateway URL: a retry or a
/// re-transcribe names its own engine. A retry sets `keep_failed` false: its
/// clip is already kept, and a second failure must not keep another.
pub fn run_with(
    provider: Provider,
    model: &str,
    gateway_url: Option<&str>,
    samples: &[f32],
    language: Option<&str>,
    keep_failed: bool,
) -> Result<(String, UsageRecord), String> {
    let dir = failed_clips_dir().filter(|_| keep_failed);
    let keep = dir.as_deref().map(|dir| KeepClips {
        dir,
        retention: clip_retention(),
    });
    let endpoint = match endpoint_for(provider, gateway_url) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            if let Some(keep) = keep
                && !matches!(error, CloudError::TooLong)
            {
                let _ = keep_clip(keep.dir, keep.retention, provider.id(), samples);
            }
            return Err(error.message(provider));
        }
    };
    let key = key_for_request(provider);
    let catalogue = (provider == Provider::OpenRouter)
        .then(|| {
            openrouter_models()
                .into_iter()
                .find(|entry| entry.id == model)
        })
        .flatten();
    let mut call = Call::new(provider, &endpoint, model);
    call.key = key.as_deref();
    call.language = language;
    call.catalogue = catalogue.as_ref();
    run_at(&call, samples, keep)
}

/// The gateway URL setting, as applied.
pub fn gateway_url() -> Option<String> {
    chosen().gateway_url.clone()
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
    for old in clips
        .iter()
        .take(clips.len().saturating_sub(MAX_KEPT_CLIPS))
    {
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

/// The file name of a failed clip: when it failed and which engine ran it.
/// The origin is what tells a retry that audio recorded for a local model
/// is about to leave the machine.
pub fn clip_name(timestamp_ms: u64, origin: &str) -> String {
    format!("failed-{timestamp_ms}-{origin}.wav")
}

/// The time and engine in a failed clip's file name. A name without an
/// engine (`failed-<ms>.wav`, from before clips named one) was a cloud
/// engine's, as only those were kept then.
pub fn parse_clip_name(name: &str) -> Option<(u64, String)> {
    let stem = name.strip_suffix(".wav")?.strip_prefix("failed-")?;
    let (stamp, origin) = stem.split_once('-').unwrap_or((stem, "groq"));
    let origin_ok = !origin.is_empty()
        && origin
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    (origin_ok).then(|| stamp.parse().ok().map(|ms| (ms, origin.to_owned())))?
}

/// Saves a clip as `failed-<ms>-<provider>.wav` (mode 0600, in a 0700 directory), then
/// sweeps the directory. `Ok(None)` when history is off: no new clip is kept.
fn keep_clip(
    directory: &Path,
    retention: HistoryRetention,
    origin: &str,
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
    let path = directory.join(clip_name(stamp as u64, origin));
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
        let url = format!(
            "http://{}/openai/v1/audio/transcriptions",
            listener.local_addr().unwrap()
        );
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

    fn groq_run(
        url: &str,
        key: Option<&str>,
        samples: &[f32],
        keep: Option<KeepClips<'_>>,
    ) -> Result<(String, UsageRecord), String> {
        let mut call = Call::new(Provider::Groq, url, "whisper-large-v3-turbo");
        call.key = key;
        run_at(&call, samples, keep)
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
        let wav = body
            .windows(4)
            .position(|window| window == b"RIFF")
            .unwrap();
        let reader = hound::WavReader::new(Cursor::new(&body[wav..])).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 16_000, 16)
        );
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
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}/x");
        let error = transcribe_at(&url, "k", "m", None, &speech()).unwrap_err();
        assert_eq!(error.message(Provider::Groq), "can't reach Groq");
    }

    #[test]
    fn a_failed_clip_is_kept_owner_only_and_a_good_one_is_not() {
        let directory = temp("kept");
        let (url, _received) = mock(401, "{}");
        let error = groq_run(&url, Some("k"), &speech(), keep(&directory)).unwrap_err();
        assert_eq!(error, "Groq key rejected");
        let kept: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].extension().and_then(|e| e.to_str()), Some("wav"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&kept[0]).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        hound::WavReader::open(&kept[0]).unwrap();

        let good = temp("not-kept");
        let (url, _received) = mock(200, r#"{"text":"fine"}"#);
        let (text, _) = groq_run(&url, Some("k"), &speech(), keep(&good)).unwrap();
        assert_eq!(text, "fine");
        assert!(!good.exists() || fs::read_dir(&good).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&directory);
        let _ = fs::remove_dir_all(&good);
    }

    #[test]
    fn a_missing_key_fails_without_a_request_and_keeps_the_clip() {
        let directory = temp("nokey");
        let error =
            groq_run("http://127.0.0.1:1/x", None, &speech(), keep(&directory)).unwrap_err();
        assert_eq!(
            error,
            "no Groq key set \u{2014} run `wordink keys set groq`"
        );
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
        assert_eq!(
            serde_json::to_string(&EngineChoice::Groq).unwrap(),
            "\"groq\""
        );
        assert_eq!(
            serde_json::from_str::<EngineChoice>("\"local\"").unwrap(),
            EngineChoice::Local
        );
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
        let (_, usage) = groq_run(&url, Some("k"), &speech(), keep(&directory)).unwrap();
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
        assert!(
            key.is_none() && stale,
            "an empty cache is stale and answers without resolving"
        );
        cache.refresh(|| Resolution {
            key: Some("k-1".into()),
            unreadable: false,
        });
        // The keyring timed out or is backing off: the key stays.
        cache.refresh(|| Resolution {
            key: None,
            unreadable: true,
        });
        assert_eq!(
            cache.peek(Duration::from_secs(30)).0.as_deref(),
            Some("k-1")
        );
        // A clean answer of "none" does clear it.
        cache.refresh(|| Resolution {
            key: None,
            unreadable: false,
        });
        assert!(cache.peek(Duration::from_secs(30)).0.is_none());
        // Unreadable with nothing held stays empty, not stale forever.
        cache.refresh(|| Resolution {
            key: None,
            unreadable: true,
        });
        assert!(!cache.peek(Duration::from_secs(30)).1);
    }

    #[test]
    fn a_model_less_engine_never_falls_back_to_local_while_the_key_is_unreadable() {
        for resolves in [true, false] {
            assert_eq!(
                route_for_engine(EngineChoice::Auto, resolves, true),
                Some(Provider::Groq)
            );
            assert_eq!(
                route_for_engine(EngineChoice::Groq, resolves, true),
                Some(Provider::Groq)
            );
            // Choosing Local still means no audio leaves the machine.
            assert_eq!(route_for_engine(EngineChoice::Local, resolves, true), None);
        }
        assert_eq!(route_for_engine(EngineChoice::Auto, false, false), None);
        assert_eq!(
            route_for_engine(EngineChoice::Auto, true, false),
            Some(Provider::Groq)
        );
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
            let error =
                groq_run("http://127.0.0.1:1/x", key, &too_long, keep(&directory)).unwrap_err();
            assert_eq!(error, "Recording too long for Groq (about 13 min max)");
            assert!(!directory.exists(), "a too-long clip is not kept");
        }
    }

    #[test]
    fn without_a_support_dir_a_failed_clip_is_not_kept_anywhere() {
        let error = groq_run("http://127.0.0.1:1/x", None, &speech(), None).unwrap_err();
        assert!(error.starts_with("no Groq key set"));
        assert!(
            !std::env::temp_dir()
                .join("wordink-failed-dictations")
                .exists()
        );
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
            aged_clip(
                &directory,
                &format!("failed-{:03}.wav", 100 + index),
                Duration::from_secs(60),
            );
        }
        sweep_clips(&directory, HistoryRetention::Forever, SystemTime::now());
        let left: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
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
        let kept = keep_clip(&directory, HistoryRetention::Week, "groq", &speech())
            .unwrap()
            .unwrap();
        assert!(
            kept.exists() && !stale.exists(),
            "the write swept the expired clip"
        );
        let off = temp("off");
        assert!(
            keep_clip(&off, HistoryRetention::Off, "groq", &speech())
                .unwrap()
                .is_none()
        );
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

    // ---- U7: more providers ----

    fn catalogue_fixture() -> serde_json::Value {
        serde_json::json!({ "data": [
            { "id": "vendor/text-only", "name": "Text Only",
              "architecture": { "input_modalities": ["text"] },
              "pricing": { "prompt": "0.000001", "completion": "0.000002" } },
            { "id": "vendor/vision", "name": "Vision",
              "architecture": { "input_modalities": ["text", "image"] },
              "pricing": { "prompt": "0.000001", "completion": "0.000002" } },
            { "id": "google/gemini-2.5-flash", "name": "Google: Gemini 2.5 Flash",
              "architecture": { "input_modalities": ["text", "image", "audio"] },
              "pricing": { "prompt": "0.0000003", "completion": "0.0000025", "audio": "0.000001" } },
            { "id": "vendor/audio-free", "name": "Audio Free",
              "architecture": { "input_modalities": ["audio"] },
              "pricing": { "prompt": "0", "completion": "0" } },
            { "id": "vendor/audio-dynamic", "name": "Audio Dynamic",
              "architecture": { "input_modalities": ["audio", "text"] },
              "pricing": { "prompt": "-1", "completion": "-1" } },
            { "id": "vendor/no-architecture", "pricing": {} },
        ] })
    }

    #[test]
    fn the_openrouter_catalogue_keeps_only_models_that_take_audio_in() {
        let models = openrouter::parse_catalogue(&catalogue_fixture());
        let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "google/gemini-2.5-flash",
                "vendor/audio-dynamic",
                "vendor/audio-free"
            ]
        );
        let gemini = &models[0];
        assert_eq!(gemini.name, "Google: Gemini 2.5 Flash");
        assert_eq!(gemini.prompt_usd, 0.0000003);
        assert_eq!(gemini.completion_usd, 0.0000025);
        assert_eq!(gemini.audio_usd, Some(0.000001));
        // A price set per request (-1) reads as unknown, not negative.
        assert_eq!(models[1].prompt_usd, 0.0);
        assert_eq!(models[1].audio_usd, None);
    }

    #[test]
    fn the_openrouter_request_is_a_chat_completion_with_an_input_audio_part() {
        let reply = r#"{"choices":[{"message":{"content":" hello there "}}],"usage":{"prompt_tokens":120,"completion_tokens":3,"prompt_tokens_details":{"audio_tokens":100}}}"#;
        let (url, received) = mock(200, reply);
        let samples = speech();
        let (text, tokens) =
            transcribe_openrouter(&url, "or-key", "google/gemini-2.5-flash", &samples).unwrap();
        assert_eq!(text, "hello there");
        assert_eq!(
            tokens,
            openrouter::Tokens {
                prompt: 120,
                audio: 100,
                completion: 3
            }
        );
        let (head, body) = received.recv().unwrap();
        let head = head.to_ascii_lowercase();
        assert!(head.contains("authorization: bearer or-key"));
        assert!(head.contains("content-type: application/json"));
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["model"], "google/gemini-2.5-flash");
        let content = &request["messages"][0]["content"];
        assert_eq!(request["messages"][0]["role"], "user");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], openrouter::INSTRUCTION);
        assert_eq!(content[1]["type"], "input_audio");
        assert_eq!(content[1]["input_audio"]["format"], "wav");
        assert_eq!(
            content[1]["input_audio"]["data"].as_str().unwrap(),
            openrouter::base64(&wav_bytes(&samples)),
            "the clip is the whole WAV, base64"
        );
        assert!(
            content[1]["input_audio"]["data"]
                .as_str()
                .unwrap()
                .starts_with("UklGR"),
            "RIFF"
        );
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (input, expected) in [
            ("", ""),
            ("M", "TQ=="),
            ("Ma", "TWE="),
            ("Man", "TWFu"),
            ("Many", "TWFueQ=="),
        ] {
            assert_eq!(openrouter::base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn an_openrouter_call_is_priced_from_the_catalogue_with_the_audio_rate() {
        let model = openrouter::Model {
            id: "m/x".into(),
            name: "X".into(),
            prompt_usd: 1e-6,
            completion_usd: 4e-6,
            audio_usd: Some(1e-5),
        };
        let tokens = openrouter::Tokens {
            prompt: 120,
            audio: 100,
            completion: 5,
        };
        // 20 text tokens + 100 audio tokens + 5 out.
        let expected = 20.0 * 1e-6 + 100.0 * 1e-5 + 5.0 * 4e-6;
        assert!((openrouter::cost(&model, tokens) - expected).abs() < 1e-12);
        let flat = openrouter::Model {
            audio_usd: None,
            ..model.clone()
        };
        assert!((openrouter::cost(&flat, tokens) - (120.0 * 1e-6 + 5.0 * 4e-6)).abs() < 1e-12);

        // And a whole run records it as the usage's cost.
        let reply = r#"{"choices":[{"message":{"content":"one two"}}],"usage":{"prompt_tokens":120,"completion_tokens":5,"prompt_tokens_details":{"audio_tokens":100}}}"#;
        let (url, _received) = mock(200, reply);
        let mut call = Call::new(Provider::OpenRouter, &url, "m/x");
        call.key = Some("k");
        call.catalogue = Some(&model);
        let (_, usage) = run_at(&call, &speech(), None).unwrap();
        assert_eq!(
            (usage.provider.as_str(), usage.model.as_str(), usage.words),
            ("openrouter", "m/x", 2)
        );
        assert!((usage.cost_usd - expected).abs() < 1e-12);
    }

    #[test]
    fn the_catalogue_is_cached_for_a_day_and_falls_back_to_the_bundled_list() {
        let directory = temp("catalogue");
        let cache = directory.join(openrouter::CACHE_FILE);
        let fetched = || Ok(openrouter::parse_catalogue(&catalogue_fixture()));
        let hour = 60 * 60 * 1000;
        // Nothing cached: fetched, and written.
        let (models, source) = openrouter::load(&cache, 1_000, fetched);
        assert_eq!((models.len(), source), (3, openrouter::Source::Fetched));
        // Under a day old: the cache answers and nothing is fetched.
        let (_, source) = openrouter::load(&cache, 1_000 + 23 * hour, || {
            panic!("fetched a fresh cache")
        });
        assert_eq!(source, openrouter::Source::Cached);
        // A day old: fetched again.
        let (_, source) = openrouter::load(&cache, 1_000 + 25 * hour, fetched);
        assert_eq!(source, openrouter::Source::Fetched);
        // A failed fetch keeps the stale cache.
        let (models, source) =
            openrouter::load(&cache, 1_000 + 60 * hour, || Err("offline".into()));
        assert_eq!((models.len(), source), (3, openrouter::Source::Cached));
        // No cache and a failed fetch: the bundled list, and it takes audio.
        let (models, source) =
            openrouter::load(&directory.join("none.json"), 5, || Err("offline".into()));
        assert_eq!(source, openrouter::Source::Bundled);
        assert!(models.iter().any(|m| m.id == openrouter::DEFAULT_MODEL));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn openai_takes_a_multipart_request_for_its_transcribe_model_and_whisper_1() {
        assert_eq!(
            Provider::OpenAi.endpoint(),
            Some("https://api.openai.com/v1/audio/transcriptions")
        );
        assert_eq!(Provider::OpenAi.default_model(), "gpt-4o-mini-transcribe");
        assert!(Provider::OpenAi.accepts_model("whisper-1"));
        assert!(!Provider::OpenAi.accepts_model("nova-3"));
        for model in ["gpt-4o-mini-transcribe", "whisper-1"] {
            let (url, received) = mock(200, r#"{"text":"hi there"}"#);
            let mut call = Call::new(Provider::OpenAi, &url, model);
            call.key = Some("sk-test");
            call.language = Some("en");
            let (text, usage) = run_at(&call, &speech(), None).unwrap();
            assert_eq!(
                (text.as_str(), usage.provider.as_str(), usage.model.as_str()),
                ("hi there", "openai", model)
            );
            let (head, body) = received.recv().unwrap();
            let head = head.to_ascii_lowercase();
            assert!(head.contains("authorization: bearer sk-test"));
            assert!(head.contains("content-type: multipart/form-data; boundary="));
            let body = String::from_utf8_lossy(&body);
            assert!(body.contains(&format!("name=\"model\"\r\n\r\n{model}")));
            assert!(body.contains("name=\"language\"\r\n\r\nen"));
            assert!(body.contains("filename=\"dictation.wav\""));
        }
    }

    #[test]
    fn deepgram_takes_the_wav_as_the_body_to_listen_with_nova_3() {
        assert_eq!(
            Provider::Deepgram.endpoint(),
            Some("https://api.deepgram.com/v1/listen")
        );
        assert_eq!(
            deepgram_url("https://api.deepgram.com/v1/listen", "nova-3", Some("fr")),
            "https://api.deepgram.com/v1/listen?model=nova-3&smart_format=true&language=fr"
        );
        let reply =
            r#"{"results":{"channels":[{"alternatives":[{"transcript":" good morning "}]}]}}"#;
        let (url, received) = mock(200, reply);
        let samples = speech();
        let mut call = Call::new(Provider::Deepgram, &url, "nova-3");
        call.key = Some("dg-key");
        let (text, usage) = run_at(&call, &samples, None).unwrap();
        assert_eq!(
            (text.as_str(), usage.provider.as_str(), usage.words),
            ("good morning", "deepgram", 2)
        );
        let (head, body) = received.recv().unwrap();
        let lower = head.to_ascii_lowercase();
        assert!(head.starts_with("POST /openai/v1/audio/transcriptions?model=nova-3&smart_format=true&detect_language=true "), "{head}");
        assert!(lower.contains("authorization: token dg-key"));
        assert!(lower.contains("content-type: audio/wav"));
        assert_eq!(body, wav_bytes(&samples), "the raw WAV, not a form");
    }

    #[test]
    fn every_provider_reads_http_failures_as_the_same_pill_texts() {
        for provider in [
            Provider::OpenRouter,
            Provider::OpenAi,
            Provider::Deepgram,
            Provider::Gateway,
        ] {
            let name = provider.name();
            for (status, expected) in [
                (401, format!("{name} key rejected")),
                (403, format!("{name} key rejected")),
                (402, format!("{name} account is out of credit")),
                (429, format!("{name} rate limit \u{2014} try again shortly")),
                (502, format!("{name} is unavailable")),
                (
                    400,
                    format!("{name} could not transcribe this clip (HTTP 400)"),
                ),
            ] {
                let (url, _received) = mock(status, "{}");
                let mut call = Call::new(provider, &url, provider.default_model());
                call.key = Some("k");
                let error = run_at(&call, &speech(), None).unwrap_err();
                assert_eq!(error, expected, "{name} HTTP {status}");
            }
            // A reply that isn't the provider's shape is unreadable, not text.
            let (url, _received) = mock(200, r#"{"unexpected":true}"#);
            let mut call = Call::new(provider, &url, provider.default_model());
            call.key = Some("k");
            assert_eq!(
                run_at(&call, &speech(), None).unwrap_err(),
                format!("{name} sent an unreadable reply")
            );
        }
    }

    #[test]
    fn a_failed_call_to_any_provider_keeps_its_clip_named_for_the_engine() {
        let directory = temp("origin");
        let (url, _received) = mock(401, "{}");
        let mut call = Call::new(Provider::Deepgram, &url, "nova-3");
        call.key = Some("k");
        run_at(&call, &speech(), keep(&directory)).unwrap_err();
        let name = fs::read_dir(&directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        let (_, origin) = parse_clip_name(name.to_str().unwrap()).unwrap();
        assert_eq!(origin, "deepgram");
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn clip_names_round_trip_and_old_names_read_as_groq() {
        assert_eq!(clip_name(1700, "openrouter"), "failed-1700-openrouter.wav");
        assert_eq!(
            parse_clip_name("failed-1700-openrouter.wav"),
            Some((1700, "openrouter".into()))
        );
        assert_eq!(
            parse_clip_name("failed-1700-local.wav"),
            Some((1700, "local".into()))
        );
        assert_eq!(
            parse_clip_name("failed-1700.wav"),
            Some((1700, "groq".into()))
        );
        for bad in [
            "failed-x.wav",
            "other-1.wav",
            "failed-1-.wav",
            "failed-1-a/b.wav",
            "failed-1.txt",
            "../failed-1.wav",
        ] {
            assert_eq!(parse_clip_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_gateway_refuses_plain_http_unless_the_host_is_this_machine() {
        let refused = |url: &str| gateway::endpoint(url).unwrap_err();
        assert_eq!(
            refused("http://gateway.example.com"),
            CloudError::GatewayNotSecure
        );
        assert_eq!(
            refused("http://192.168.1.20:8787"),
            CloudError::GatewayNotSecure
        );
        assert_eq!(
            refused("http://localhost.evil.com"),
            CloudError::GatewayNotSecure
        );
        // A user part can't make a remote host look local.
        assert_eq!(
            refused("http://localhost@evil.com/"),
            CloudError::BadGatewayUrl
        );
        assert_eq!(
            refused("https://user:pw@gw.example.com"),
            CloudError::BadGatewayUrl
        );
        assert_eq!(refused("ftp://gw.example.com"), CloudError::BadGatewayUrl);
        assert_eq!(refused("not a url"), CloudError::BadGatewayUrl);
        assert_eq!(refused("  "), CloudError::NoGateway);
        assert_eq!(
            gateway::endpoint("http://localhost:8787").unwrap(),
            "http://localhost:8787/v1/audio/transcriptions"
        );
        assert_eq!(
            gateway::endpoint("http://127.0.0.1:8787/").unwrap(),
            "http://127.0.0.1:8787/v1/audio/transcriptions"
        );
        assert_eq!(
            gateway::endpoint("https://gw.example.com").unwrap(),
            "https://gw.example.com/v1/audio/transcriptions"
        );
        assert_eq!(
            gateway::endpoint("https://gw.example.com/wordink/v1?x=1#y").unwrap(),
            "https://gw.example.com/wordink/v1/audio/transcriptions"
        );
        assert_eq!(
            gateway::endpoint("https://gw.example.com/v1/audio/transcriptions").unwrap(),
            "https://gw.example.com/v1/audio/transcriptions"
        );
        assert_eq!(
            CloudError::GatewayNotSecure.message(Provider::Gateway),
            "the gateway URL must be https (http only for localhost or 127.0.0.1)"
        );
    }

    #[test]
    fn a_refused_gateway_url_sends_nothing_and_a_local_one_sends_the_bearer_token() {
        // Refused before any request: port 1 would read "can't reach".
        let error = run_with(
            Provider::Gateway,
            "gateway",
            Some("http://example.com"),
            &speech(),
            None,
            false,
        )
        .unwrap_err();
        assert!(
            error.starts_with("the gateway URL must be https"),
            "{error}"
        );
        let error =
            run_with(Provider::Gateway, "gateway", None, &speech(), None, false).unwrap_err();
        assert!(error.starts_with("no gateway URL set"), "{error}");

        let (mock_url, received) = mock(200, r#"{"text":"from the gateway"}"#);
        let base = mock_url.split("/openai").next().unwrap();
        let endpoint = gateway::endpoint(base).unwrap();
        assert!(endpoint.ends_with("/v1/audio/transcriptions"));
        // The mock answers any path; the call goes where the gateway says.
        let mut call = Call::new(Provider::Gateway, &endpoint, "gateway");
        call.key = Some("wdk_device");
        let (text, usage) = run_at(&call, &speech(), None).unwrap();
        assert_eq!(
            (text.as_str(), usage.provider.as_str(), usage.cost_usd),
            ("from the gateway", "gateway", 0.0)
        );
        let (head, _) = received.recv().unwrap();
        assert!(head.starts_with("POST /v1/audio/transcriptions "), "{head}");
        assert!(
            head.to_ascii_lowercase()
                .contains("authorization: bearer wdk_device")
        );
        assert_eq!(
            CloudError::NoKey.message(Provider::Gateway),
            "no gateway token set \u{2014} run `wordink keys set gateway`"
        );
    }

    #[test]
    fn the_engine_choice_names_every_provider_and_routes_to_it() {
        for (choice, name, provider) in [
            (EngineChoice::Openrouter, "openrouter", Provider::OpenRouter),
            (EngineChoice::Openai, "openai", Provider::OpenAi),
            (EngineChoice::Deepgram, "deepgram", Provider::Deepgram),
            (EngineChoice::Gateway, "gateway", Provider::Gateway),
        ] {
            assert_eq!(
                serde_json::to_string(&choice).unwrap(),
                format!("\"{name}\"")
            );
            assert_eq!(choice.provider(), Some(provider));
            assert_eq!(provider.id(), name);
            // A named engine runs without a Groq key, and with one.
            for groq in [true, false] {
                assert_eq!(route_for(choice, groq), Some(provider));
                assert_eq!(route_for_engine(choice, groq, true), Some(provider));
            }
        }
        // Groq stays the Auto default.
        assert_eq!(route_for(EngineChoice::Auto, true), Some(Provider::Groq));
        assert_eq!(route_for(EngineChoice::Auto, false), None);
        assert_eq!(EngineChoice::default(), EngineChoice::Auto);
    }

    #[test]
    fn the_chosen_model_applies_only_to_a_provider_that_takes_it() {
        assert_eq!(model_for(Provider::OpenAi, Some("whisper-1")), "whisper-1");
        assert_eq!(model_for(Provider::OpenAi, None), "gpt-4o-mini-transcribe");
        // A model left over from another provider is ignored.
        assert_eq!(
            model_for(Provider::OpenAi, Some("google/gemini-2.5-flash")),
            "gpt-4o-mini-transcribe"
        );
        assert_eq!(model_for(Provider::Deepgram, Some("whisper-1")), "nova-3");
        assert_eq!(
            model_for(Provider::OpenRouter, Some("google/gemini-2.5-flash")),
            "google/gemini-2.5-flash"
        );
        assert_eq!(
            model_for(Provider::OpenRouter, Some("whisper-1")),
            openrouter::DEFAULT_MODEL
        );
        assert_eq!(
            model_for(Provider::Groq, Some("whisper-1")),
            "whisper-large-v3-turbo"
        );
        assert!(
            models_for(Provider::OpenRouter)
                .iter()
                .any(|(id, _)| id == openrouter::DEFAULT_MODEL)
        );
        assert_eq!(models_for(Provider::OpenAi).len(), 2);
    }

    #[test]
    fn the_bundled_prices_cover_openai_and_deepgram_per_minute() {
        let per_minute = |provider, model| estimate_cost(provider, model, 60.0).unwrap();
        assert!((per_minute("openai", "gpt-4o-mini-transcribe") - 0.003).abs() < 1e-12);
        assert!((per_minute("openai", "whisper-1") - 0.006).abs() < 1e-12);
        assert!((per_minute("deepgram", "nova-3") - 0.0043).abs() < 1e-12);
        // No ten-second floor outside Groq: 30 s is half a minute.
        assert!((estimate_cost("deepgram", "nova-3", 30.0).unwrap() - 0.00215).abs() < 1e-12);
        assert_eq!(
            estimate_cost("openrouter", "google/gemini-2.5-flash", 60.0),
            None
        );
        assert_eq!(estimate_cost("gateway", "gateway", 60.0), None);
    }
}
