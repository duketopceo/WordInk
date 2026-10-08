//! The dictation session state machine.
//!
//! `Idle -> RequestingMic -> Listening -> Transcribing -> Idle`, where a
//! session can end in `Error`, and a press from `Error` starts over through
//! `Idle`. Push-to-talk and toggle only differ in which button input stops a
//! recording; the transitions and effects are the same.

use crate::audio::{to_pcm16, Resampler};
use crate::effects::{Effect, Event};
use crate::error::ErrorCode;
use crate::providers::{Ids, Progress, Provider, Run};

/// Recordings shorter than this end in [`ErrorCode::NoSpeech`].
pub const MIN_SPEECH_MS: u64 = 300;

/// Level windows at or above this RMS (about -40 dBFS) count as speech. A
/// recording with no such window ends in [`ErrorCode::NoSpeech`].
pub const SPEECH_RMS_THRESHOLD: f32 = 0.01;

/// Default longest utterance. Capture stops on its own here, as if the
/// button were released, and what was captured is transcribed. 60 s of
/// 16 kHz PCM16 WAV is about 1.9 MB, under the relay's 2 MB body cap.
pub const MAX_UTTERANCE_MS: u64 = 60_000;

/// Level events per second of audio.
const LEVEL_HZ: u32 = 20;

/// Streaming providers get audio in chunks of at least this many per second
/// (50 ms), rather than one message per AudioWorklet block.
const STREAM_CHUNKS_HZ: u32 = 20;

/// How the button maps to start and stop (R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Hold to talk: press starts, release stops.
    PushToTalk,
    /// Tap to start, tap again to stop. Release is ignored.
    Toggle,
}

/// Session configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionConfig {
    /// Button behavior.
    pub mode: Mode,
    /// Speech provider. Audio is resampled to its declared rate (KTD4).
    pub provider: Provider,
    /// Custom vocabulary or prompt hint (R15), passed to the provider in its
    /// own form: Whisper and OpenAI prompt, Deepgram keyterms (split on
    /// commas and newlines), or the host provider's start effect.
    pub hint: Option<String>,
    /// Longest utterance in milliseconds of captured audio, after which the
    /// session stops capture and transcribes. Defaults to
    /// [`MAX_UTTERANCE_MS`].
    pub max_utterance_ms: u64,
}

impl SessionConfig {
    /// A configuration with no hint.
    pub fn new(mode: Mode, provider: Provider) -> Self {
        Self {
            mode,
            provider,
            hint: None,
            max_utterance_ms: MAX_UTTERANCE_MS,
        }
    }
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
    run: Option<Box<dyn Run>>,
    /// A final the run reported while still listening.
    early_final: Option<String>,
    ids: Ids,
}

impl Session {
    /// Creates an idle session.
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            state: State::Idle,
            recording: None,
            run: None,
            early_final: None,
            ids: Ids::default(),
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
                let rate = self.config.provider.capabilities().sample_rate;
                self.recording = Some(Recording::new(sample_rate, rate));
                self.set(State::Listening, &mut fx);
                let mut run = self.config.provider.run(self.config.hint.as_deref());
                run.start(&mut self.ids, &mut fx);
                self.run = Some(run);
            }
            (Event::MicDenied, State::RequestingMic) => {
                self.set(State::Error(ErrorCode::MicDenied), &mut fx)
            }
            (event, State::Listening | State::Transcribing) => {
                let progress = match self.run.as_mut() {
                    Some(run) => run.event(event, &mut self.ids, &mut fx),
                    None => None,
                };
                match progress {
                    // A final before release (a streaming provider that ended
                    // on its own) is shown as interim text; release then
                    // completes with it, as the run has nothing more to give.
                    Some(Progress::Interim(text)) => fx.push(Effect::Interim { text }),
                    Some(Progress::Final(text)) if self.state == State::Listening => {
                        self.early_final = Some(text.clone());
                        fx.push(Effect::Interim { text })
                    }
                    Some(Progress::Final(text)) => self.complete(text, &mut fx),
                    Some(Progress::Failed(code)) => self.fail(code, &mut fx),
                    None => {}
                }
            }
            _ => {}
        }
        fx
    }

    /// Feeds captured mono Float32 audio at the rate given in
    /// [`Event::MicGranted`]. Returns [`Effect::Level`] meter updates, about
    /// 20 per second of audio. Ignored outside `Listening`.
    ///
    /// Once the recording reaches [`SessionConfig::max_utterance_ms`], the
    /// rest of `samples` is dropped and the session stops as if released,
    /// so the effects can also include [`Effect::StopMic`], a state change
    /// and the provider's request.
    pub fn push_audio(&mut self, samples: &[f32]) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(rec) = self.recording.as_mut() {
            let max = self.config.max_utterance_ms * u64::from(rec.input_rate) / 1000;
            let room = max.saturating_sub(rec.input_samples);
            let take = samples
                .len()
                .min(usize::try_from(room).unwrap_or(usize::MAX));
            rec.push(&samples[..take], &mut fx);
            let streaming = self.config.provider.capabilities().streaming;
            if let Some(run) = self.run.as_mut().filter(|_| streaming) {
                if let Some(chunk) = rec.stream_chunk() {
                    run.audio(chunk, &mut fx);
                }
            }
            if rec.input_samples >= max {
                self.stop(&mut fx);
            }
        }
        fx
    }

    fn set(&mut self, state: State, fx: &mut Vec<Effect>) {
        self.state = state;
        fx.push(Effect::State(state));
    }

    fn start(&mut self, fx: &mut Vec<Effect>) {
        self.early_final = None;
        self.set(State::RequestingMic, fx);
        fx.push(Effect::RequestMic);
    }

    /// Ends capture: transcribe if there is usable speech, else `NoSpeech`.
    fn stop(&mut self, fx: &mut Vec<Effect>) {
        fx.push(Effect::StopMic);
        let finished = self.recording.take().and_then(Recording::finish);
        if let Some(text) = self.early_final.take() {
            return self.complete(text, fx);
        }
        match finished {
            Some((pcm, streamed)) if self.run.is_some() => {
                self.set(State::Transcribing, fx);
                if let Some(run) = self.run.as_mut() {
                    run.finish(&pcm, &pcm[streamed..], &mut self.ids, fx);
                }
            }
            _ => self.fail(ErrorCode::NoSpeech, fx),
        }
    }

    /// Ends the session with the provider's final text.
    fn complete(&mut self, text: String, fx: &mut Vec<Effect>) {
        self.end_run(fx);
        let text = text.trim();
        if text.is_empty() {
            self.set(State::Error(ErrorCode::NoSpeech), fx);
        } else {
            fx.push(Effect::Final { text: text.into() });
            self.set(State::Idle, fx);
        }
    }

    /// Ends the session in an error, stopping capture if it is running.
    fn fail(&mut self, code: ErrorCode, fx: &mut Vec<Effect>) {
        if self.recording.take().is_some() {
            fx.push(Effect::StopMic);
        }
        self.end_run(fx);
        self.set(State::Error(code), fx);
    }

    fn end_run(&mut self, fx: &mut Vec<Effect>) {
        if let Some(mut run) = self.run.take() {
            run.end(fx);
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
    /// How much of `pcm` has been handed to a streaming provider.
    streamed: usize,
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
            streamed: 0,
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

    /// Audio not yet streamed, once at least one chunk's worth is ready.
    fn stream_chunk(&mut self) -> Option<&[i16]> {
        let min = (self.target_rate / STREAM_CHUNKS_HZ).max(1) as usize;
        let start = self.streamed;
        (self.pcm.len() - start >= min).then(|| {
            self.streamed = self.pcm.len();
            &self.pcm[start..]
        })
    }

    /// Finishes the recording, returning the PCM16 utterance at the target
    /// rate and how much of it was streamed, or `None` if it holds no usable
    /// speech.
    fn finish(mut self) -> Option<(Vec<i16>, usize)> {
        if self.window_len > 0 {
            self.close_window();
        }
        let duration_ms = self.input_samples * 1000 / u64::from(self.input_rate);
        if duration_ms < MIN_SPEECH_MS || !self.heard_speech {
            return None;
        }
        self.resample(|r, out| r.flush(out));
        Some((self.pcm, self.streamed))
    }
}
