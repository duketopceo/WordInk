//! Error codes a session can end in.

/// Why a session ended in [`State::Error`](crate::State::Error).
///
/// Each code is actionable on its own, so a host can show a specific message
/// (R9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// The user or browser refused microphone access.
    MicDenied,
    /// The recording was shorter than [`MIN_SPEECH_MS`](crate::session::MIN_SPEECH_MS)
    /// or contained only silence (no provider was called), or the provider
    /// returned an empty transcript.
    NoSpeech,
    /// The provider rejected the credential: a missing, invalid or expired
    /// key or token (HTTP 401/403, or a socket that never opened).
    AuthFailed,
    /// The provider is rate limiting this key (HTTP 429).
    RateLimited,
    /// The provider is unreachable or failed (network error, HTTP 5xx).
    ProviderDown,
    /// The provider could not decode the audio (HTTP 400/413/415).
    BadAudio,
}
