//! What the core asks the host to do, and what the host reports back.
//!
//! The core never performs I/O. Every call into a [`Session`](crate::Session)
//! returns a list of [`Effect`]s, which the host performs in order. Results
//! of that I/O come back as [`Event`]s.
//!
//! Network effects carry an `id`. The host echoes it on every event that
//! answers the effect, so the core can ignore results from a request or
//! socket it has already abandoned.

use crate::error::ErrorCode;
use crate::session::State;

/// Something the host must do or show, in the order returned.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Effect {
    /// The session moved to a new state. Hosts drive their UI from this.
    State(State),
    /// Ask for microphone access and start capture. Answer with
    /// [`Event::MicGranted`] or [`Event::MicDenied`], then feed audio through
    /// [`Session::push_audio`](crate::Session::push_audio).
    RequestMic,
    /// Stop capture and release the microphone, including a request that is
    /// still pending.
    StopMic,
    /// Current input level while listening, as the RMS of mono Float32
    /// samples (0.0 to 1.0), emitted about 20 times per second of audio.
    Level {
        /// Root-mean-square amplitude of the last level window.
        rms: f32,
    },
    /// Text so far, from a streaming provider (R8). Each interim replaces
    /// the previous one: it is the whole transcript so far, not a delta.
    Interim {
        /// Transcript so far.
        text: String,
    },
    /// The finished text for the session.
    Final {
        /// Transcribed text.
        text: String,
    },
    /// Send an HTTP request. Answer with [`Event::HttpResponse`] or, if no
    /// response arrived (network error, timeout), [`Event::HttpFailed`].
    HttpRequest(HttpRequest),
    /// Open a WebSocket offering these subprotocols. Answer with
    /// [`Event::WsOpened`], then [`Event::WsMessage`] for each message and
    /// [`Event::WsClosed`] when it closes, including when it fails to open.
    WsOpen {
        /// Socket id, echoed on its events.
        id: u32,
        /// `wss://` URL.
        url: String,
        /// `Sec-WebSocket-Protocol` values, in order. Some providers carry
        /// the short-lived credential here.
        protocols: Vec<String>,
    },
    /// Send a message on an open socket.
    WsSend {
        /// Socket id from [`Effect::WsOpen`].
        id: u32,
        /// Message payload.
        data: WsData,
    },
    /// Close the socket. No further events for it are needed.
    WsClose {
        /// Socket id from [`Effect::WsOpen`].
        id: u32,
    },
    /// Start a host-implemented provider (KTD5) for one utterance. Answer
    /// with [`Event::HostProviderResult`].
    HostProviderStart {
        /// Provider id given in [`HostProvider`](crate::providers::HostProvider).
        id: String,
        /// Rate of the audio that follows, the provider's declared rate.
        sample_rate: u32,
        /// Vocabulary or prompt hint (R15).
        hint: Option<String>,
    },
    /// Deliver mono PCM16 audio to the host provider.
    HostProviderAudio {
        /// Provider id.
        id: String,
        /// Samples at the rate given in [`Effect::HostProviderStart`].
        samples: Vec<i16>,
    },
    /// The utterance is complete; the host provider should produce its final
    /// result.
    HostProviderFinish {
        /// Provider id.
        id: String,
    },
    /// Drop the utterance; no result is wanted.
    HostProviderCancel {
        /// Provider id.
        id: String,
    },
}

/// An HTTP request for the host to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Request id, echoed on the response.
    pub id: u32,
    /// HTTP method.
    pub method: String,
    /// Absolute URL.
    pub url: String,
    /// Extra request headers. The host sets `Content-Type` for the body.
    pub headers: Vec<(String, String)>,
    /// Request body.
    pub body: HttpBody,
}

/// An HTTP request body.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpBody {
    /// `multipart/form-data`, in part order. Browser hosts build a
    /// `FormData` from it.
    Multipart(Vec<Part>),
}

/// One `multipart/form-data` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// Field name.
    pub name: String,
    /// Field value.
    pub value: PartValue,
}

/// A multipart field value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartValue {
    /// A text field.
    Text(String),
    /// A file field.
    File {
        /// File name sent with the part.
        filename: String,
        /// Media type of `data`.
        content_type: String,
        /// File contents.
        data: Vec<u8>,
    },
}

/// A WebSocket message payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsData {
    /// A text frame.
    Text(String),
    /// A binary frame.
    Binary(Vec<u8>),
}

/// What a host-implemented provider reports back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostResult {
    /// Transcript so far (streaming providers only).
    Interim {
        /// Transcript so far.
        text: String,
    },
    /// The finished transcript.
    Final {
        /// Transcribed text.
        text: String,
    },
    /// The provider failed.
    Error {
        /// Why.
        code: ErrorCode,
    },
}

/// Input from the user, or the result of host I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// The dictation button went down.
    Press,
    /// The dictation button came up. Ignored in [`Mode::Toggle`](crate::Mode::Toggle).
    Release,
    /// Microphone capture started.
    MicGranted {
        /// Capture rate in Hz of the audio that will be pushed. Must be non-zero.
        sample_rate: u32,
    },
    /// Microphone access was refused.
    MicDenied,
    /// An HTTP response arrived, of any status.
    HttpResponse {
        /// Id from [`HttpRequest::id`].
        id: u32,
        /// HTTP status code.
        status: u16,
        /// Response body as text.
        body: String,
    },
    /// An HTTP request got no response (network error or timeout).
    HttpFailed {
        /// Id from [`HttpRequest::id`].
        id: u32,
    },
    /// A socket from [`Effect::WsOpen`] opened.
    WsOpened {
        /// Socket id.
        id: u32,
    },
    /// A socket received a message.
    WsMessage {
        /// Socket id.
        id: u32,
        /// Message payload.
        data: WsData,
    },
    /// A socket closed, or failed to open (browsers report close code 1006).
    WsClosed {
        /// Socket id.
        id: u32,
        /// WebSocket close code.
        code: u16,
    },
    /// A host-implemented provider reported a result.
    HostProviderResult(HostResult),
}
