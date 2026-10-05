//! Error codes a session can end in.

/// Why a session ended in [`State::Error`](crate::State::Error).
///
/// Each code is actionable on its own, so a host can show a specific message
/// (R9). Provider failure codes are added alongside the provider protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// The user or browser refused microphone access.
    MicDenied,
    /// The recording was shorter than [`MIN_SPEECH_MS`](crate::session::MIN_SPEECH_MS)
    /// or contained only silence. No provider was called.
    NoSpeech,
}
