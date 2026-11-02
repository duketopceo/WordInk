//! OpenRouter audio models (R19): the catalogue, its daily cache, the chat
//! completion request that carries the clip as an `input_audio` part, and
//! the price of a call from the catalogue's `pricing` field.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
pub const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
/// How long a fetched catalogue is used before the next fetch.
pub const CACHE_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
pub const CACHE_FILE: &str = "openrouter-models.json";
/// The model dictation uses until one is chosen.
pub const DEFAULT_MODEL: &str = "google/gemini-2.5-flash-lite";

/// What the model is told. Fixed: the transcript is the whole reply.
pub const INSTRUCTION: &str = "Transcribe this audio exactly as spoken. Reply with only the transcript text: no quotes, no commentary, no translation. If there is no speech, reply with nothing.";

/// One catalogue model that takes audio in. Prices are US dollars per token.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub prompt_usd: f64,
    pub completion_usd: f64,
    /// The audio-input price per token, when it differs from `prompt_usd`.
    pub audio_usd: Option<f64>,
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
            Some(Model {
                prompt_usd: price(pricing.and_then(|p| p.get("prompt"))).unwrap_or(0.0),
                completion_usd: price(pricing.and_then(|p| p.get("completion"))).unwrap_or(0.0),
                audio_usd: price(pricing.and_then(|p| p.get("audio"))),
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
    };
    vec![
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
pub fn chat_body(model: &str, wav: &[u8]) -> Value {
    json!({
        "model": model,
        "temperature": 0,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": INSTRUCTION },
                { "type": "input_audio", "input_audio": { "data": base64(wav), "format": "wav" } },
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

/// Estimated cost of a call, from the model's catalogue prices.
pub fn cost(model: &Model, tokens: Tokens) -> f64 {
    let audio = tokens.audio.min(tokens.prompt);
    let text = tokens.prompt - audio;
    text as f64 * model.prompt_usd
        + audio as f64 * model.audio_usd.unwrap_or(model.prompt_usd)
        + tokens.completion as f64 * model.completion_usd
}

#[derive(Deserialize, Serialize)]
struct Cache {
    fetched_at_ms: u64,
    models: Vec<Model>,
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
    if let Some(cached) = &cached
        && now_ms.saturating_sub(cached.fetched_at_ms) < CACHE_MAX_AGE_MS
    {
        return (cached.models.clone(), Source::Cached);
    }
    match fetch() {
        Ok(models) if !models.is_empty() => {
            if let Ok(json) = serde_json::to_vec(&Cache {
                fetched_at_ms: now_ms,
                models: models.clone(),
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
