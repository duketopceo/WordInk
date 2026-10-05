//! OpenAI realtime transcription over a WebSocket.
//!
//! The browser authenticates with an `ek_` client secret carried in a
//! subprotocol. That path is unverified against the live API (plan Risks);
//! the URL and subprotocols are data in [`Effect::WsOpen`], so a host could
//! swap the transport without changing this protocol logic.

use super::json::{self, Value};
use super::{base64, pcm_bytes, Capabilities, Ids, Progress, Run, Socket, SocketEvent};
use crate::effects::{Effect, Event, WsData};
use crate::error::ErrorCode;

pub(crate) const CAPABILITIES: Capabilities = Capabilities {
    streaming: true,
    sample_rate: 24_000,
};

const URL: &str = "wss://api.openai.com/v1/realtime?intent=transcription";

/// OpenAI realtime transcription configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenAi {
    /// Short-lived `ek_` client secret minted by the relay (KTD3), or a key
    /// in localhost dev mode.
    pub secret: String,
    /// Transcription model.
    pub model: String,
}

impl OpenAi {
    /// Default model. Model names are in flux, so this is configurable.
    pub const DEFAULT_MODEL: &'static str = "gpt-4o-transcribe";

    /// OpenAI with a client secret.
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            model: Self::DEFAULT_MODEL.into(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct OpenAiRun {
    socket: Socket,
    session_update: String,
    /// Deltas received so far.
    text: String,
}

impl OpenAiRun {
    pub(crate) fn new(o: &OpenAi, hint: Option<&str>) -> Self {
        let protocols = vec![
            "realtime".to_owned(),
            format!("openai-insecure-api-key.{}", o.secret),
        ];
        // Manual turns: the session commits once, on release.
        let mut update = String::from(
            r#"{"type":"transcription_session.update","session":{"input_audio_format":"pcm16","input_audio_transcription":{"model":"#,
        );
        json::quote(&o.model, &mut update);
        if let Some(hint) = hint {
            update.push_str(r#","prompt":"#);
            json::quote(hint, &mut update);
        }
        update.push_str(r#"},"turn_detection":null}}"#);
        Self {
            socket: Socket::new(URL.to_owned(), vec![protocols]),
            session_update: update,
            text: String::new(),
        }
    }

    fn append(&mut self, pcm: &[i16], fx: &mut Vec<Effect>) {
        if pcm.is_empty() {
            return;
        }
        let msg = format!(
            r#"{{"type":"input_audio_buffer.append","audio":"{}"}}"#,
            base64(&pcm_bytes(pcm))
        );
        self.socket.send(WsData::Text(msg), fx);
    }

    fn message(&mut self, v: &Value, fx: &mut Vec<Effect>) -> Option<Progress> {
        match v.get("type").str()? {
            "conversation.item.input_audio_transcription.delta" => {
                self.text.push_str(v.get("delta").str()?);
                Some(Progress::Interim(self.text.clone()))
            }
            "conversation.item.input_audio_transcription.completed" => {
                self.socket.close(fx);
                Some(Progress::Final(v.get("transcript").str()?.to_owned()))
            }
            "error" => Some(Progress::Failed(error_code(v.get("error")))),
            _ => None,
        }
    }
}

/// Maps an `error` event's details to an error code.
fn error_code(e: &Value) -> ErrorCode {
    let code = e.get("code").str().unwrap_or("");
    let kind = e.get("type").str().unwrap_or("");
    match (code, kind) {
        ("input_audio_buffer_commit_empty", _) => ErrorCode::NoSpeech,
        ("invalid_api_key", _) | (_, "authentication_error") => ErrorCode::AuthFailed,
        ("rate_limit_exceeded", _) | (_, "rate_limit_error") => ErrorCode::RateLimited,
        (c, _) if c.contains("audio") => ErrorCode::BadAudio,
        _ => ErrorCode::ProviderDown,
    }
}

impl Run for OpenAiRun {
    fn start(&mut self, ids: &mut Ids, fx: &mut Vec<Effect>) {
        self.socket.connect(ids, fx);
        let update = core::mem::take(&mut self.session_update);
        self.socket.send(WsData::Text(update), fx);
    }

    fn audio(&mut self, pcm: &[i16], fx: &mut Vec<Effect>) {
        self.append(pcm, fx);
    }

    fn finish(&mut self, _all: &[i16], rest: &[i16], _ids: &mut Ids, fx: &mut Vec<Effect>) {
        self.append(rest, fx);
        let commit = r#"{"type":"input_audio_buffer.commit"}"#.to_owned();
        self.socket.send(WsData::Text(commit), fx);
    }

    fn event(&mut self, event: Event, ids: &mut Ids, fx: &mut Vec<Effect>) -> Option<Progress> {
        match self.socket.handle(event, ids, fx) {
            SocketEvent::Text(text) => self.message(&json::parse(&text)?, fx),
            other => Socket::failure(&other),
        }
    }

    fn end(&mut self, fx: &mut Vec<Effect>) {
        self.socket.close(fx);
    }
}
