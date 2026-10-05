//! What the core asks the host to do, and what the host reports back.
//!
//! The core never performs I/O. Every call into a [`Session`](crate::Session)
//! returns a list of [`Effect`]s, which the host performs in order. Results
//! of that I/O come back as [`Event`]s.

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
    /// Transcribe this utterance and answer with [`Event::TranscriptFinal`].
    ///
    /// This is the provider seam: provider adapters turn it into concrete
    /// request or socket effects.
    Transcribe(Utterance),
    /// The finished text for the session.
    Final {
        /// Transcribed text.
        text: String,
    },
}

/// A recorded utterance, resampled to the session's target rate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    /// Mono PCM16 samples.
    pub samples: Vec<i16>,
    /// Sample rate of `samples` in Hz.
    pub sample_rate: u32,
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
    /// The transcriber finished.
    TranscriptFinal {
        /// Transcribed text.
        text: String,
    },
}
