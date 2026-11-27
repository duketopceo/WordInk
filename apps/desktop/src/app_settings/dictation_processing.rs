//! What happens to a dictation's text before it pastes: Global's steps and
//! the modes chosen by application or website.

use serde::{Deserialize, Serialize};

use super::{KeyboardModelChoice, TextReplacement};
use crate::dictation::vocabulary::{VocabularyEntry, serialize_saved};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationProcessingSettings {
    pub default_mode: DictationMode,
    pub modes: Vec<DictationMode>,
}

impl Default for DictationProcessingSettings {
    fn default() -> Self {
        Self {
            default_mode: DictationMode {
                name: "Global".into(),
                ..Default::default()
            },
            modes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationMode {
    pub name: String,
    pub applications: Vec<String>,
    pub browser_hosts: Vec<String>,
    pub replacements: Vec<TextReplacement>,
    pub transformations: Vec<String>,
    pub post_processing: DictationPostProcessing,
    /// The speech model and language this mode selects while it matches the
    /// foreground app (Labs › Switch model with app). None means don't switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<KeyboardModelChoice>,
    /// Names heard and spelled here ahead of the shared vocabulary.
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_saved"
    )]
    pub vocabulary: Vec<VocabularyEntry>,
}

/// Which service rewrites a mode's transcript after rule-based cleanup.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PostProcessingBackend {
    /// OpenCode's loopback HTTP API (default).
    #[default]
    OpenCode,
    /// A local OpenAI-compatible chat server (`mlx_vlm.server`, `llama-server`, …).
    LocalOpenAi,
}

/// Persisted post-processing settings for one mode (OpenCode or a local model).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationPostProcessing {
    pub enabled: bool,
    pub prompt: String,
    /// Optional `provider/model` key from the OpenCode catalog.
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
    pub backend: PostProcessingBackend,
    /// Root URL of a local OpenAI-compatible server when `backend` is `local_openai`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_base_url: Option<String>,
    /// Model name the local server expects in `chat/completions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_model: Option<String>,
}

impl Default for DictationPostProcessing {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt: "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.".into(),
            model: None,
            variant: None,
            deadline_seconds: 30,
            backend: PostProcessingBackend::OpenCode,
            local_base_url: None,
            local_model: None,
        }
    }
}

impl DictationPostProcessing {
    /// Whether this mode calls a configured local OpenAI-compatible server.
    pub fn uses_local_openai(&self) -> bool {
        self.enabled
            && self.backend == PostProcessingBackend::LocalOpenAi
            && self
                .local_base_url
                .as_deref()
                .is_some_and(|url| !url.trim().is_empty())
            && self
                .local_model
                .as_deref()
                .is_some_and(|model| !model.trim().is_empty())
    }
}
