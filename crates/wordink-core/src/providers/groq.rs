//! Groq Whisper: batch transcription by multipart POST (KTD10).

use super::{http_error, Capabilities, Ids, Progress, Run};
use crate::audio::encode_wav;
use crate::effects::{Effect, Event, HttpBody, HttpRequest, Part, PartValue};
use crate::error::ErrorCode;

pub(crate) const CAPABILITIES: Capabilities = Capabilities {
    streaming: false,
    sample_rate: 16_000,
};

/// Groq's OpenAI-compatible endpoint, for dev-key mode.
const DIRECT_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

/// How the core reaches Groq.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// The developer's `@wordink/server` relay, which holds the key (KTD3).
    /// Audio is posted to `{endpoint}/groq/transcriptions`.
    Relay {
        /// Relay base URL.
        endpoint: String,
    },
    /// A key sent straight to Groq. Hosts allow this on localhost only.
    DevKey(String),
}

/// Groq Whisper configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Groq {
    /// Relay or direct key.
    pub credential: Credential,
    /// Whisper model.
    pub model: String,
    /// ISO-639-1 language, or `None` to auto-detect.
    pub language: Option<String>,
}

impl Groq {
    /// Default model.
    pub const DEFAULT_MODEL: &'static str = "whisper-large-v3-turbo";

    /// Groq through the developer's relay.
    pub fn relay(endpoint: impl Into<String>) -> Self {
        Self::with(Credential::Relay {
            endpoint: endpoint.into(),
        })
    }

    /// Groq called directly with a key (dev mode).
    pub fn dev_key(key: impl Into<String>) -> Self {
        Self::with(Credential::DevKey(key.into()))
    }

    fn with(credential: Credential) -> Self {
        Self {
            credential,
            model: Self::DEFAULT_MODEL.into(),
            language: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct GroqRun {
    config: Groq,
    hint: Option<String>,
    request: Option<u32>,
}

impl GroqRun {
    pub(crate) fn new(g: &Groq, hint: Option<&str>) -> Self {
        Self {
            config: g.clone(),
            hint: hint.map(str::to_owned),
            request: None,
        }
    }
}

fn text(name: &str, value: &str) -> Part {
    Part {
        name: name.into(),
        value: PartValue::Text(value.into()),
    }
}

impl Run for GroqRun {
    fn finish(&mut self, all: &[i16], _rest: &[i16], ids: &mut Ids, fx: &mut Vec<Effect>) {
        let (url, headers) = match &self.config.credential {
            Credential::Relay { endpoint } => (
                format!("{}/groq/transcriptions", endpoint.trim_end_matches('/')),
                Vec::new(),
            ),
            Credential::DevKey(key) => (
                DIRECT_URL.to_owned(),
                vec![("Authorization".to_owned(), format!("Bearer {key}"))],
            ),
        };
        let mut parts = vec![
            Part {
                name: "file".into(),
                value: PartValue::File {
                    filename: "audio.wav".into(),
                    content_type: "audio/wav".into(),
                    data: encode_wav(all, CAPABILITIES.sample_rate),
                },
            },
            text("model", &self.config.model),
            text("response_format", "text"),
        ];
        if let Some(hint) = &self.hint {
            parts.push(text("prompt", hint));
        }
        if let Some(language) = &self.config.language {
            parts.push(text("language", language));
        }
        let id = ids.next();
        self.request = Some(id);
        fx.push(Effect::HttpRequest(HttpRequest {
            id,
            method: "POST".into(),
            url,
            headers,
            body: HttpBody::Multipart(parts),
        }));
    }

    fn event(&mut self, event: Event, _ids: &mut Ids, _fx: &mut Vec<Effect>) -> Option<Progress> {
        let progress = match event {
            Event::HttpResponse { id, status, body } if Some(id) == self.request => {
                if status == 200 {
                    Progress::Final(body)
                } else {
                    Progress::Failed(http_error(status))
                }
            }
            Event::HttpFailed { id } if Some(id) == self.request => {
                Progress::Failed(ErrorCode::ProviderDown)
            }
            _ => return None,
        };
        self.request = None;
        Some(progress)
    }

    fn end(&mut self, _fx: &mut Vec<Effect>) {
        self.request = None;
    }
}
