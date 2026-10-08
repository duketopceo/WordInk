//! Deepgram live transcription over a WebSocket.
//!
//! The browser passes the token as the `token` subprotocol. That path is
//! unverified against the live API (plan Risks), so a socket that fails to
//! open is retried once with the `bearer` subprotocol.

use super::json::{self, Value};
use super::{pcm_bytes, url_encode, Capabilities, Ids, Progress, Run, Socket, SocketEvent};
use crate::effects::{Effect, Event, WsData};

pub(crate) const CAPABILITIES: Capabilities = Capabilities {
    streaming: true,
    sample_rate: 16_000,
};

/// Deepgram live configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Deepgram {
    /// Short-lived JWT minted by the relay (KTD3), or a key in localhost
    /// dev mode.
    pub token: String,
    /// Model.
    pub model: String,
}

impl Deepgram {
    /// Default model.
    pub const DEFAULT_MODEL: &'static str = "nova-3";

    /// Deepgram with a token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            model: Self::DEFAULT_MODEL.into(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct DeepgramRun {
    socket: Socket,
    /// Finalized segments, in order.
    finals: Vec<String>,
    /// `CloseStream` was sent: a normal close now completes the transcript.
    closing: bool,
}

impl DeepgramRun {
    pub(crate) fn new(d: &Deepgram, hint: Option<&str>) -> Self {
        let mut url = format!(
            "wss://api.deepgram.com/v1/listen?encoding=linear16&sample_rate={}&channels=1\
             &interim_results=true&model=",
            CAPABILITIES.sample_rate
        );
        url_encode(&d.model, &mut url);
        let terms = hint.into_iter().flat_map(|h| h.split([',', '\n']));
        for term in terms.map(str::trim).filter(|t| !t.is_empty()) {
            url.push_str("&keyterm=");
            url_encode(term, &mut url);
        }
        let candidates = ["token", "bearer"]
            .iter()
            .map(|scheme| vec![(*scheme).to_owned(), d.token.clone()])
            .collect();
        Self {
            socket: Socket::new(url, candidates),
            finals: Vec::new(),
            closing: false,
        }
    }

    fn transcript(&self, partial: Option<&str>) -> String {
        let mut parts: Vec<&str> = self.finals.iter().map(String::as_str).collect();
        parts.extend(partial);
        parts.join(" ")
    }

    fn message(&mut self, v: &Value) -> Option<Progress> {
        if v.get("type").str()? != "Results" {
            return None;
        }
        let segment = v
            .get("channel")
            .get("alternatives")
            .at(0)
            .get("transcript")
            .str()?
            .trim();
        let is_final = v.get("is_final").bool().unwrap_or(false);
        if segment.is_empty() {
            return None;
        }
        let text = if is_final {
            self.finals.push(segment.to_owned());
            self.transcript(None)
        } else {
            self.transcript(Some(segment))
        };
        Some(Progress::Interim(text))
    }
}

impl Run for DeepgramRun {
    fn start(&mut self, ids: &mut Ids, fx: &mut Vec<Effect>) {
        self.socket.connect(ids, fx);
    }

    fn audio(&mut self, pcm: &[i16], fx: &mut Vec<Effect>) {
        self.socket.send(WsData::Binary(pcm_bytes(pcm)), fx);
    }

    fn finish(&mut self, _all: &[i16], rest: &[i16], _ids: &mut Ids, fx: &mut Vec<Effect>) {
        if !rest.is_empty() {
            self.audio(rest, fx);
        }
        let close = r#"{"type":"CloseStream"}"#.to_owned();
        self.socket.send(WsData::Text(close), fx);
        self.closing = true;
    }

    fn event(&mut self, event: Event, ids: &mut Ids, fx: &mut Vec<Effect>) -> Option<Progress> {
        match self.socket.handle(event, ids, fx) {
            SocketEvent::Text(text) => self.message(&json::parse(&text)?),
            // Deepgram flushes its last results and closes normally after
            // `CloseStream`; the transcript is complete then.
            SocketEvent::Closed(1000) if self.closing => {
                Some(Progress::Final(self.transcript(None)))
            }
            other => Socket::failure(&other),
        }
    }

    fn end(&mut self, fx: &mut Vec<Effect>) {
        self.socket.close(fx);
    }
}
