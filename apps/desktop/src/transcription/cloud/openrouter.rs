//! OpenRouter audio models (R19): the catalogue, its daily cache, and two
//! ways to transcribe with it. Audio-capable chat models take the clip as an
//! `input_audio` part of a chat completion and are priced from the
//! catalogue's per-token `pricing`. Dedicated transcription models
//! (MAI-Transcribe, Whisper, Voxtral STT, ...) take it at
//! `/audio/transcriptions` and are priced per audio hour.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
pub const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
/// The dedicated speech-to-text endpoint.
pub const TRANSCRIPTION_URL: &str = "https://openrouter.ai/api/v1/audio/transcriptions";
/// The catalogue lists transcription models only when asked for them.
pub const TRANSCRIPTION_MODELS_URL: &str =
    "https://openrouter.ai/api/v1/models?output_modalities=transcription";
/// How long a fetched catalogue is used before the next fetch.
pub const CACHE_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
pub const CACHE_FILE: &str = "openrouter-models.json";
/// The model dictation uses until one is chosen.
pub const DEFAULT_MODEL: &str = "google/gemini-2.5-flash-lite";

/// The model a meeting uses at OpenRouter until one is chosen: the most
/// accurate on meeting audio in WordInk's comparison (11.2% WER against
/// 15.3% for Groq's turbo, at $0.10 per audio hour).
pub const MEETING_MODEL: &str = "microsoft/mai-transcribe-2";

/// What the model is told. Fixed: the transcript is the whole reply.
pub const INSTRUCTION: &str = "Transcribe this audio exactly as spoken. Reply with only the transcript text: no quotes, no commentary, no translation. If there is no speech, reply with nothing.";

/// Which OpenRouter API a model is called through.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An audio-capable chat model, called through chat completions.
    #[default]
    Chat,
    /// A speech-to-text model, called at `/audio/transcriptions`.
    Transcription,
}

impl Kind {
    /// The name logs and the request log use.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Transcription => "transcription",
        }
    }
}

/// One catalogue model that takes audio in. Prices are US dollars per token
/// (chat models); a transcription model's price is `usd_per_hour`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub prompt_usd: f64,
    pub completion_usd: f64,
    /// The audio-input price per token, when it differs from `prompt_usd`.
    pub audio_usd: Option<f64>,
    #[serde(default)]
    pub kind: Kind,
    /// US dollars per hour of audio, for a transcription model that is not
    /// priced by token.
    #[serde(default)]
    pub usd_per_hour: Option<f64>,
}

/// Where a model list came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Fetched,
    Cached,
    /// The fetch failed and there was no cache: the bundled list.
    Bundled,
}

/// Token counts a reply reports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tokens {
    pub prompt: u64,
    pub audio: u64,
    pub completion: u64,
}

fn price(value: Option<&Value>) -> Option<f64> {
    let parsed = match value? {
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        Value::Number(number) => number.as_f64()?,
        _ => return None,
    };
    // OpenRouter uses -1 for prices set per request.
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed)
}

/// The audio-hour price of a transcription model from its catalogue
/// `pricing.prompt`, or `None` for one billed by token (a non-zero
/// `completion` price) or with no price.
///
/// The catalogue quotes most transcription models per audio second
/// (`0.0000833` is $0.30/hour) but MAI-Transcribe per audio hour (`0.1` is
/// $0.10/hour, confirmed against `usage.cost` of live calls). No real
/// per-second price comes near a cent, so a quoted price of a cent or more is
/// read as per hour.
pub fn hourly_price(prompt_usd: f64, completion_usd: f64) -> Option<f64> {
    if completion_usd > 0.0 || prompt_usd <= 0.0 {
        return None;
    }
    Some(if prompt_usd >= 0.01 {
        prompt_usd
    } else {
        prompt_usd * 3600.0
    })
}

/// The models in a `GET /api/v1/models` reply whose `architecture.
/// input_modalities` contains `audio`, by id.
pub fn parse_catalogue(reply: &Value) -> Vec<Model> {
    let mut models: Vec<Model> = reply
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| {
            model
                .pointer("/architecture/input_modalities")
                .and_then(Value::as_array)
                .is_some_and(|modalities| modalities.iter().any(|m| m.as_str() == Some("audio")))
        })
        .filter_map(|model| {
            let id = model.get("id")?.as_str()?.to_string();
            let pricing = model.get("pricing");
            let name = model
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let transcription = model
                .pointer("/architecture/output_modalities")
                .and_then(Value::as_array)
                .is_some_and(|modalities| {
                    modalities.iter().any(|m| m.as_str() == Some("transcription"))
                });
            let prompt_usd = price(pricing.and_then(|p| p.get("prompt"))).unwrap_or(0.0);
            let completion_usd = price(pricing.and_then(|p| p.get("completion"))).unwrap_or(0.0);
            Some(Model {
                prompt_usd,
                completion_usd,
                audio_usd: price(pricing.and_then(|p| p.get("audio"))),
                kind: if transcription {
                    Kind::Transcription
                } else {
                    Kind::Chat
                },
                usd_per_hour: transcription
                    .then(|| hourly_price(prompt_usd, completion_usd))
                    .flatten(),
                id,
                name,
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

/// Used when the catalogue can't be fetched and nothing is cached. Prices
/// are a snapshot of the catalogue taken 2026-11-02.
pub fn bundled() -> Vec<Model> {
    let model = |id: &str, name: &str, prompt: f64, completion: f64, audio: Option<f64>| Model {
        id: id.into(),
        name: name.into(),
        prompt_usd: prompt,
        completion_usd: completion,
        audio_usd: audio,
        kind: Kind::Chat,
        usd_per_hour: None,
    };
    let stt = |id: &str, name: &str, usd_per_hour: f64| Model {
        id: id.into(),
        name: name.into(),
        prompt_usd: 0.0,
        completion_usd: 0.0,
        audio_usd: None,
        kind: Kind::Transcription,
        usd_per_hour: Some(usd_per_hour),
    };
    vec![
        stt("microsoft/mai-transcribe-2", "Microsoft AI: MAI-Transcribe 2", 0.10),
        stt("mistralai/voxtral-mini-transcribe", "Mistral: Voxtral Mini Transcribe", 0.21),
        stt("nvidia/parakeet-tdt-0.6b-v3", "NVIDIA: Parakeet TDT 0.6B v3", 0.09),
        stt("openai/whisper-large-v3-turbo", "OpenAI: Whisper Large V3 Turbo", 0.012),
        model(
            "google/gemini-2.5-flash",
            "Google: Gemini 2.5 Flash",
            3e-7,
            2.5e-6,
            Some(1e-6),
        ),
        model(
            "google/gemini-2.5-flash-lite",
            "Google: Gemini 2.5 Flash Lite",
            1e-7,
            4e-7,
            Some(3e-7),
        ),
        model(
            "mistralai/voxtral-small-24b-2507",
            "Mistral: Voxtral Small 24B 2507",
            1e-7,
            3e-7,
            Some(1e-4),
        ),
        model(
            "openai/gpt-audio-mini",
            "OpenAI: GPT Audio Mini",
            6e-7,
            2.4e-6,
            Some(6e-7),
        ),
    ]
}

/// The chat completion body: the fixed instruction and the clip as base64
/// WAV in an `input_audio` part.
pub fn chat_body(model: &str, audio: &[u8], format: super::UploadFormat) -> Value {
    json!({
        "model": model,
        "temperature": 0,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": INSTRUCTION },
                { "type": "input_audio", "input_audio": { "data": base64(audio), "format": format.extension() } },
            ],
        }],
    })
}

/// The transcript and token counts of a chat completion reply.
pub fn parse_reply(reply: &Value) -> Option<(String, Tokens)> {
    let content = reply.pointer("/choices/0/message/content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        // Some providers answer with content parts.
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => return None,
    };
    let count = |pointer: &str| reply.pointer(pointer).and_then(Value::as_u64).unwrap_or(0);
    Some((
        text.trim().to_string(),
        Tokens {
            prompt: count("/usage/prompt_tokens"),
            audio: count("/usage/prompt_tokens_details/audio_tokens"),
            completion: count("/usage/completion_tokens"),
        },
    ))
}

/// What a transcription request asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Detail {
    /// `{ text, usage }`: the fastest reply, and what dictation uses.
    Plain,
    /// `verbose_json`: adds segment timestamps (and word timestamps when the
    /// provider has them), which a meeting uses. Not every provider takes it.
    Verbose,
}

/// The `/audio/transcriptions` JSON body: the model, and the clip as base64
/// WAV in `input_audio`.
pub fn transcription_body(
    model: &str,
    wav: &[u8],
    language: Option<&str>,
    detail: Detail,
) -> Value {
    let mut body = json!({
        "model": model,
        "input_audio": { "data": base64(wav), "format": "wav" },
    });
    if let Some(language) = language {
        body["language"] = json!(language);
    }
    if detail == Detail::Verbose {
        body["response_format"] = json!("verbose_json");
    }
    body
}

/// A timed piece of a transcript, in milliseconds from the clip's start.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// What a transcription reply holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcription {
    pub text: String,
    /// Empty unless the request was `verbose_json` and the provider timed it.
    pub segments: Vec<Timed>,
    pub words: Vec<Timed>,
    /// Audio seconds the provider billed (`usage.seconds`).
    pub billed_seconds: Option<f64>,
    /// What the provider charged (`usage.cost`), in US dollars.
    pub cost_usd: Option<f64>,
    pub total_tokens: Option<u64>,
}

fn seconds_to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

fn timed(items: Option<&Value>, text_key: &str) -> Vec<Timed> {
    items
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| {
            // `audio_event` entries are sounds ("(laughter)"), not speech.
            item.get("type").and_then(Value::as_str).is_none_or(|kind| kind == "word")
        })
        .filter_map(|item| {
            let text = item.get(text_key)?.as_str()?.trim();
            let start = item.get("start")?.as_f64()?;
            let end = item.get("end")?.as_f64()?;
            (!text.is_empty() && start.is_finite() && end.is_finite()).then(|| Timed {
                start_ms: seconds_to_ms(start),
                end_ms: seconds_to_ms(end),
                text: text.to_owned(),
            })
        })
        .collect()
}

/// The transcript and usage of an `/audio/transcriptions` reply, or `None`
/// when it has no `text`.
pub fn parse_transcription(reply: &Value) -> Option<Transcription> {
    let text = reply.get("text")?.as_str()?.trim().to_owned();
    let usage = |key: &str| reply.pointer(&format!("/usage/{key}"));
    Some(Transcription {
        text,
        segments: timed(reply.get("segments"), "text"),
        words: timed(reply.get("words"), "word"),
        billed_seconds: usage("seconds")
            .and_then(Value::as_f64)
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0),
        cost_usd: usage("cost")
            .and_then(Value::as_f64)
            .filter(|cost| cost.is_finite() && *cost >= 0.0),
        total_tokens: usage("total_tokens").and_then(Value::as_u64),
    })
}

/// What an OpenRouter error body (`{"error":{"code":..,"message":..}}`)
/// says: its code. The provider's message is not kept, so it can never reach
/// the pill or a log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiError {
    pub code: Option<i64>,
}

pub fn parse_error(body: &str) -> Option<ApiError> {
    let value: Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    Some(ApiError {
        code: error.get("code").and_then(Value::as_i64),
    })
}

/// Estimated cost of a transcription call when the reply carried none:
/// the model's catalogue hourly price over the audio.
pub fn transcription_cost(model: &Model, audio_seconds: f64) -> Option<f64> {
    model
        .usd_per_hour
        .map(|per_hour| audio_seconds.max(0.0) * per_hour / 3600.0)
}

/// Estimated cost of a call, from the model's catalogue prices.
pub fn cost(model: &Model, tokens: Tokens) -> f64 {
    let audio = tokens.audio.min(tokens.prompt);
    let text = tokens.prompt - audio;
    text as f64 * model.prompt_usd
        + audio as f64 * model.audio_usd.unwrap_or(model.prompt_usd)
        + tokens.completion as f64 * model.completion_usd
}

/// Bumped when the cache gains something older caches lack, so they are
/// fetched again at once rather than after a day. 2: transcription models.
const CACHE_VERSION: u32 = 2;

#[derive(Deserialize, Serialize)]
struct Cache {
    fetched_at_ms: u64,
    models: Vec<Model>,
    #[serde(default)]
    version: u32,
}

fn read_cache(path: &Path) -> Option<Cache> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// The model list: the cache while it is under a day old, else a fetch
/// (written to the cache), else a stale cache, else the bundled list.
pub fn load(
    cache: &Path,
    now_ms: u64,
    fetch: impl FnOnce() -> Result<Vec<Model>, String>,
) -> (Vec<Model>, Source) {
    let cached = read_cache(cache).filter(|cache| !cache.models.is_empty());
    // A cache from before transcription models were listed has none, and is
    // fetched again at once rather than after a day.
    if let Some(cached) = &cached
        && now_ms.saturating_sub(cached.fetched_at_ms) < CACHE_MAX_AGE_MS
        && cached.version >= CACHE_VERSION
    {
        return (cached.models.clone(), Source::Cached);
    }
    match fetch() {
        Ok(models) if !models.is_empty() => {
            if let Ok(json) = serde_json::to_vec(&Cache {
                fetched_at_ms: now_ms,
                models: models.clone(),
                version: CACHE_VERSION,
            }) {
                if let Some(parent) = cache.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(cache, json);
            }
            (models, Source::Fetched)
        }
        _ => match cached {
            Some(cached) => (cached.models, Source::Cached),
            None => (bundled(), Source::Bundled),
        },
    }
}

/// The cache or the bundled list, never the network.
pub fn cached_or_bundled(cache: &Path) -> Vec<Model> {
    read_cache(cache)
        .map(|cache| cache.models)
        .filter(|models| !models.is_empty())
        .unwrap_or_else(bundled)
}

/// The audio-capable chat models and the transcription models: two lists,
/// one request each. A failed transcription list leaves the chat models, so
/// the picker never ends up with nothing.
pub fn fetch_all(chat_url: &str, transcription_url: &str) -> Result<Vec<Model>, String> {
    let mut models = fetch(chat_url)?;
    match fetch(transcription_url) {
        Ok(more) => models.extend(more),
        Err(error) => tracing::debug!(%error, "OpenRouter transcription catalogue not fetched"),
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(models)
}

/// `GET` the catalogue at `url`.
pub fn fetch(url: &str) -> Result<Vec<Model>, String> {
    let agent = crate::net::agent(url, |config| {
        config.timeout_global(Some(std::time::Duration::from_secs(20)))
    });
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| crate::net::reason(&error))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    let reply: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(parse_catalogue(&reply))
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
