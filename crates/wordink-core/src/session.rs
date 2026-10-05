//! The dictation session state machine.
//!
//! `Idle -> RequestingMic -> Listening -> Transcribing -> Idle`, where a
//! session can end in `Error`, and a press from `Error` starts over through
//! `Idle`. Push-to-talk and toggle only differ in which button input stops a
//! recording; the transitions and effects are the same.

use crate::audio::{to_pcm16, Resampler};
use crate::effects::{Effect, Event, Utterance};
use crate::error::ErrorCode;

/// Recordings shorter than this end in [`ErrorCode::NoSpeech`].
pub const MIN_SPEECH_MS: u64 = 300;

/// Level windows at or above this RMS (about -40 dBFS) count as speech. A
/// recording with no such window ends in [`ErrorCode::NoSpeech`].
pub const SPEECH_RMS_THRESHOLD: f32 = 0.01;

/// Level events per second of audio.
const LEVEL_HZ: u32 = 20;

/// How the button maps to start and stop (R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Hold to talk: press starts, release stops.
    PushToTalk,
    /// Tap to start, tap again to stop. Release is ignored.
    Toggle,
}

/// Session configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    /// Button behavior.
    pub mode: Mode,
    /// Rate in Hz the utterance is resampled to: 16000 or 24000 (KTD4).
    pub target_sample_rate: u32,
}

/// Visible session state (R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Ready to start.
    Idle,
    /// Waiting for the host to open the microphone.
    RequestingMic,
    /// Capturing audio.
    Listening,
    /// Waiting for the transcript.
    Transcribing,
    /// The session failed. A press starts over.
    Error(ErrorCode),
}

/// A dictation engine, reusable across many utterances.
///
/// Call [`handle`](Self::handle) for button input and I/O results and
/// [`push_audio`](Self::push_audio) for captured audio, then perform the
/// returned [`Effect`]s in order. Inputs that don't apply to the current
/// state (a stale result, a release in toggle mode) return no effects.
#[derive(Debug)]
pub struct Session {
    config: SessionConfig,
    state: State,
    recording: Option<Recording>,
}

impl Session {
    /// Creates an idle session.
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            state: State::Idle,
            recording: None,
        }
    }

    /// Current state.
    pub fn state(&self) -> State {
        self.state
    }

    /// Feeds a button input or I/O result and returns the effects to perform.
    pub fn handle(&mut self, event: Event) -> Vec<Effect> {
        let mut fx = Vec::new();
        match (event, self.state) {
            (Event::Press, State::Idle) => self.start(&mut fx),
            (Event::Press, State::Error(_)) => {
                self.set(State::Idle, &mut fx);
                self.start(&mut fx);
            }
            (Event::Press, State::RequestingMic | State::Listening)
                if self.config.mode == Mode::Toggle =>
            {
                self.stop(&mut fx)
            }
            (Event::Release, State::RequestingMic | State::Listening)
                if self.config.mode == Mode::PushToTalk =>
            {
                self.stop(&mut fx)
            }
            (Event::MicGranted { sample_rate }, State::RequestingMic) => {
                self.recording = Some(Recording::new(sample_rate, self.config.target_sample_rate));
                self.set(State::Listening, &mut fx);
            }
            (Event::MicDenied, State::RequestingMic) => {
                self.set(State::Error(ErrorCode::MicDenied), &mut fx)
            }
            (Event::TranscriptFinal { text }, State::Transcribing) => {
                fx.push(Effect::Final { text });
                self.set(State::Idle, &mut fx);
            }
            _ => {}
        }
        fx
    }

    /// Feeds captured mono Float32 audio at the rate given in
    /// [`Event::MicGranted`]. Returns [`Effect::Level`] meter updates, about
    /// 20 per second of audio. Ignored outside `Listening`.
    pub fn push_audio(&mut self, samples: &[f32]) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(rec) = self.recording.as_mut() {
            rec.push(samples, &mut fx);
        }
        fx
    }

    fn set(&mut self, state: State, fx: &mut Vec<Effect>) {
        self.state = state;
        fx.push(Effect::State(state));
    }

    fn start(&mut self, fx: &mut Vec<Effect>) {
        self.set(State::RequestingMic, fx);
        fx.push(Effect::RequestMic);
    }

    /// Ends capture: transcribe if there is usable speech, else `NoSpeech`.
    fn stop(&mut self, fx: &mut Vec<Effect>) {
        fx.push(Effect::StopMic);
        match self.recording.take().and_then(Recording::finish) {
            Some(utterance) => {
                self.set(State::Transcribing, fx);
                fx.push(Effect::Transcribe(utterance));
            }
            None => self.set(State::Error(ErrorCode::NoSpeech), fx),
        }
    }
}

/// Audio state for one `Listening` phase.
#[derive(Debug)]
struct Recording {
    input_rate: u32,
    target_rate: u32,
    input_samples: u64,
    resampler: Resampler,
    scratch: Vec<f32>,
    pcm: Vec<i16>,
    level_window: u32,
    window_sum: f64,
    window_len: u32,
    heard_speech: bool,
}

impl Recording {
    fn new(input_rate: u32, target_rate: u32) -> Self {
        Self {
            input_rate,
            target_rate,
            input_samples: 0,
            resampler: Resampler::new(input_rate, target_rate),
            scratch: Vec::new(),
            pcm: Vec::new(),
            level_window: (input_rate / LEVEL_HZ).max(1),
            window_sum: 0.0,
            window_len: 0,
            heard_speech: false,
        }
    }

    fn push(&mut self, samples: &[f32], fx: &mut Vec<Effect>) {
        self.input_samples += samples.len() as u64;
        self.resample(|r, out| r.process(samples, out));
        for &s in samples {
            self.window_sum += f64::from(s) * f64::from(s);
            self.window_len += 1;
            if self.window_len == self.level_window {
                let rms = self.close_window();
                fx.push(Effect::Level { rms });
            }
        }
    }

    fn resample(&mut self, run: impl FnOnce(&mut Resampler, &mut Vec<f32>)) {
        self.scratch.clear();
        run(&mut self.resampler, &mut self.scratch);
        self.pcm.extend(to_pcm16(&self.scratch));
    }

    /// Ends the current level window and returns its RMS.
    fn close_window(&mut self) -> f32 {
        let rms = (self.window_sum / f64::from(self.window_len)).sqrt() as f32;
        self.heard_speech |= rms >= SPEECH_RMS_THRESHOLD;
        self.window_sum = 0.0;
        self.window_len = 0;
        rms
    }

    /// Finishes the recording, or `None` if it holds no usable speech.
    fn finish(mut self) -> Option<Utterance> {
        if self.window_len > 0 {
            self.close_window();
        }
        let duration_ms = self.input_samples * 1000 / u64::from(self.input_rate);
        if duration_ms < MIN_SPEECH_MS || !self.heard_speech {
            return None;
        }
        self.resample(|r, out| r.flush(out));
        Some(Utterance {
            samples: self.pcm,
            sample_rate: self.target_rate,
        })
    }
}
