//! Provider protocols (R10, R13).
//!
//! A [`Provider`] is plain configuration data. When listening starts, the
//! session turns it into a run that speaks the provider's protocol through
//! effects and events: a multipart POST for Groq, a WebSocket for OpenAI
//! and Deepgram, and audio-delivery effects for a host-implemented provider.
//! Switching providers changes only that configuration: the session's
//! inputs and its non-provider effects stay the same.
//!
//! Credentials are data the host supplies. The core never fetches them: for
//! streaming providers the host mints a short-lived token through its relay
//! (KTD3) before starting the session and puts it in the config.

mod deepgram;
mod groq;
mod host;
mod json;
mod openai;

pub use deepgram::Deepgram;
pub use groq::{Credential, Groq};
pub use host::HostProvider;
pub use openai::OpenAi;

use core::fmt::Debug;

use crate::effects::{Effect, Event, WsData};
use crate::error::ErrorCode;

/// What a provider can do, used by the session to shape audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// The provider takes audio while the user speaks and returns interim
    /// text (R8). Batch providers get the whole utterance on release.
    pub streaming: bool,
    /// Rate in Hz the provider takes mono PCM16 at. Must be non-zero.
    pub sample_rate: u32,
}

/// A speech provider configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Provider {
    /// Groq Whisper, batch only (the default, KTD10).
    Groq(Groq),
    /// OpenAI realtime transcription, streaming.
    OpenAi(OpenAi),
    /// Deepgram live, streaming.
    Deepgram(Deepgram),
    /// A provider the host implements, such as the local engine (KTD5).
    Host(HostProvider),
}

impl Provider {
    /// What this provider can do.
    pub fn capabilities(&self) -> Capabilities {
        match self {
            Provider::Groq(_) => groq::CAPABILITIES,
            Provider::OpenAi(_) => openai::CAPABILITIES,
            Provider::Deepgram(_) => deepgram::CAPABILITIES,
            Provider::Host(h) => h.capabilities,
        }
    }

    /// Starts a run for one utterance with the session's hint.
    pub(crate) fn run(&self, hint: Option<&str>) -> Box<dyn Run> {
        let hint = hint.map(str::trim).filter(|h| !h.is_empty());
        match self {
            Provider::Groq(g) => Box::new(groq::GroqRun::new(g, hint)),
            Provider::OpenAi(o) => Box::new(openai::OpenAiRun::new(o, hint)),
            Provider::Deepgram(d) => Box::new(deepgram::DeepgramRun::new(d, hint)),
            Provider::Host(h) => Box::new(host::HostRun::new(h, hint)),
        }
    }
}

/// What a run reports to the session after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Progress {
    /// Transcript so far.
    Interim(String),
    /// The finished transcript.
    Final(String),
    /// The run failed.
    Failed(ErrorCode),
}

/// Allocates request and socket ids, unique within a session.
#[derive(Debug, Default)]
pub(crate) struct Ids(u32);

impl Ids {
    pub(crate) fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }
}

/// One utterance's conversation with a provider.
pub(crate) trait Run: Debug {
    /// Listening started.
    fn start(&mut self, _ids: &mut Ids, _fx: &mut Vec<Effect>) {}
    /// New audio while listening (streaming providers only).
    fn audio(&mut self, _pcm: &[i16], _fx: &mut Vec<Effect>) {}
    /// The recording ended with usable speech. `all` is the whole utterance
    /// and `rest` its tail that [`audio`](Self::audio) has not seen.
    fn finish(&mut self, all: &[i16], rest: &[i16], ids: &mut Ids, fx: &mut Vec<Effect>);
    /// A provider I/O result. Events for other requests return `None`.
    fn event(&mut self, event: Event, ids: &mut Ids, fx: &mut Vec<Effect>) -> Option<Progress>;
    /// Releases any socket or host provider still in use. Idempotent.
    fn end(&mut self, fx: &mut Vec<Effect>);
}

/// Maps an HTTP error status to an error code (R9).
pub(crate) fn http_error(status: u16) -> ErrorCode {
    match status {
        401 | 403 => ErrorCode::AuthFailed,
        429 => ErrorCode::RateLimited,
        400 | 413 | 415 => ErrorCode::BadAudio,
        _ => ErrorCode::ProviderDown,
    }
}

/// The close code a host reports for a socket that never opened before its
/// connect timeout. WordInk-private: RFC 6455 reserves 4000–4999 for
/// application use, so a real provider close never carries it. It lets a
/// stalled connect be told apart from a rejected handshake (a 401 surfaces
/// to the host as a 1006 close before open).
pub const WS_CLOSE_STALL: u16 = 4408;

/// Maps the close code of a socket that had opened to an error code.
fn ws_close_error(code: u16) -> ErrorCode {
    match code {
        // IANA-registered Unauthorized and Forbidden.
        3000 | 3003 => ErrorCode::AuthFailed,
        // Try Again Later.
        1013 => ErrorCode::RateLimited,
        // Invalid payload data, policy violation (Deepgram's undecodable audio).
        1007 | 1008 => ErrorCode::BadAudio,
        _ => ErrorCode::ProviderDown,
    }
}

/// A provider socket: holds outgoing frames until it opens, and retries
/// with the next subprotocol candidate if it fails to open.
#[derive(Debug)]
pub(crate) struct Socket {
    url: String,
    candidates: Vec<Vec<String>>,
    attempt: usize,
    id: u32,
    open: bool,
    closed: bool,
    queue: Vec<WsData>,
}

/// A socket event, after lifecycle handling.
pub(crate) enum SocketEvent {
    /// Not for this socket, or fully handled (opened, retried).
    None,
    /// A text message.
    Text(String),
    /// The socket closed after opening.
    Closed(u16),
    /// The socket never opened with any candidate, with the close code the
    /// host reported. Browsers hide the handshake status, and with a
    /// reachable provider a rejected handshake almost always means a
    /// rejected credential; the host's private [`WS_CLOSE_STALL`] code marks
    /// a connect timeout instead.
    Rejected(u16),
}

impl Socket {
    pub(crate) fn new(url: String, candidates: Vec<Vec<String>>) -> Self {
        Self {
            url,
            candidates,
            attempt: 0,
            id: 0,
            open: false,
            closed: false,
            queue: Vec::new(),
        }
    }

    pub(crate) fn connect(&mut self, ids: &mut Ids, fx: &mut Vec<Effect>) {
        self.id = ids.next();
        fx.push(Effect::WsOpen {
            id: self.id,
            url: self.url.clone(),
            protocols: self.candidates[self.attempt].clone(),
        });
    }

    pub(crate) fn send(&mut self, data: WsData, fx: &mut Vec<Effect>) {
        if self.closed {
            return;
        }
        if self.open {
            fx.push(Effect::WsSend { id: self.id, data });
        } else {
            self.queue.push(data);
        }
    }

    pub(crate) fn close(&mut self, fx: &mut Vec<Effect>) {
        if !self.closed {
            self.closed = true;
            fx.push(Effect::WsClose { id: self.id });
        }
    }

    pub(crate) fn handle(
        &mut self,
        event: Event,
        ids: &mut Ids,
        fx: &mut Vec<Effect>,
    ) -> SocketEvent {
        match event {
            Event::WsOpened { id } if id == self.id && !self.closed => {
                self.open = true;
                for data in core::mem::take(&mut self.queue) {
                    fx.push(Effect::WsSend { id, data });
                }
                SocketEvent::None
            }
            Event::WsMessage {
                id,
                data: WsData::Text(text),
            } if id == self.id && !self.closed => SocketEvent::Text(text),
            Event::WsClosed { id, code } if id == self.id && !self.closed => {
                if self.open {
                    self.closed = true;
                    SocketEvent::Closed(code)
                } else if self.attempt + 1 < self.candidates.len() {
                    self.attempt += 1;
                    self.connect(ids, fx);
                    SocketEvent::None
                } else {
                    self.closed = true;
                    SocketEvent::Rejected(code)
                }
            }
            _ => SocketEvent::None,
        }
    }

    /// Lifecycle outcome to progress, for events that are not messages.
    pub(crate) fn failure(event: &SocketEvent) -> Option<Progress> {
        match event {
            SocketEvent::Closed(code) => Some(Progress::Failed(ws_close_error(*code))),
            SocketEvent::Rejected(code) => Some(Progress::Failed(match *code {
                // The host's connect timeout fired: a stall, not a verdict.
                WS_CLOSE_STALL => ErrorCode::ProviderDown,
                _ => ErrorCode::AuthFailed,
            })),
            _ => None,
        }
    }
}

/// Little-endian bytes of PCM16 samples.
pub(crate) fn pcm_bytes(pcm: &[i16]) -> Vec<u8> {
    pcm.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Percent-encodes a URL query value (everything but RFC 3986 unreserved).
pub(crate) fn url_encode(s: &str, out: &mut String) {
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b & 15)]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }

    #[test]
    fn url_encode_keeps_unreserved_only() {
        let mut out = String::new();
        url_encode("a-Z_9.~ /&é", &mut out);
        assert_eq!(out, "a-Z_9.~%20%2F%26%C3%A9");
    }
}
