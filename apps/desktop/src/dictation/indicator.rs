//! The dictation HUD's lifecycle and animation, the same on every OS: the
//! event vocabulary the listener reports, the phase and job bookkeeping, and
//! Dot scope's simulation. Each step yields the dots to draw; `rune_hud`
//! draws them with `rune_hud.wgsl` through a Rune overlay.
//!
//! Time is passed in, never read, so a recorded schedule replays exactly
//! (`rune_hud`'s frame captures).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

pub use crate::app_settings::IndicatorStyle;
use crate::rune_hud::audio_bar::{self, Bars};
use crate::rune_hud::dot_scope::{self, Pose, Scope, Stage, Tuning};
use crate::rune_hud::hud_placement::Island;

/// The loudness before any meter arrives, dBFS.
const SILENT: f32 = -120.0;
/// Live text calls a capture heard (Recording rather than Ready) once the
/// microphone is louder than this, dBFS.
const HEARD: f32 = -55.0;
/// The shortest processing look: a fast transcription still shows the field
/// turning blue before done clears it.
const PROCESSING_MIN_DURATION: Duration = Duration::from_millis(200);
/// How long done shows before the HUD collapses: a third into done's
/// 0.3 s wipe, so the collapse and the wipe end together.
const COMPLETED_VISIBLE_DURATION: Duration = Duration::from_millis(100);
const FAILED_VISIBLE_DURATION: Duration = Duration::from_millis(4_000);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub enum DictationIndicatorEvent {
    Reset,
    /// HUD Lab's or a playground story's tuning.
    ConfigureDotScope(dot_scope::Tuning),
    /// HUD Lab: Dot scope follows a synthetic voice instead of the meter
    /// until the next `Reset`.
    DemoVoice,
    Started,
    Meter {
        average: f32,
        // Dot scope follows `average`; the window's meters read
        // `input_level` instead.
        #[allow(dead_code)]
        peak: f32,
    },
    Submitted {
        job_id: u64,
    },
    Transcribing {
        job_id: u64,
    },
    Processing {
        job_id: u64,
    },
    Discarded,
    Cancelled,
    /// The speech model is loading (Live text says so).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    ModelLoading,
    /// The speech model finished loading.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    ModelReady,
    /// Bluetooth earbuds are switching to headset mode for the dictation.
    #[allow(dead_code)]
    EarbudsConnecting,
    /// The earbuds' microphone is ready.
    #[allow(dead_code)]
    EarbudsReady,
    /// The words were left on the clipboard instead of typed.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Copied,
    JobCompleted {
        job_id: u64,
    },
    JobCancelled {
        job_id: u64,
    },
    JobFailed {
        job_id: u64,
    },
    Failed,
}

impl DictationIndicatorEvent {
    /// A `Meter` event from captured samples: their RMS and absolute peak;
    /// `None` for no samples.
    pub fn meter(samples: &[f32]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sum_of_squares = 0.0;
        let mut peak = 0.0_f32;
        for sample in samples {
            sum_of_squares += sample * sample;
            peak = peak.max(sample.abs());
        }
        Some(Self::Meter {
            average: (sum_of_squares / samples.len() as f32).sqrt(),
            peak,
        })
    }
}

#[cfg(target_os = "macos")]
pub use macos::*;

/// macOS reports through a channel the main thread's pump drains into the
/// HUD (`rune_hud::macos`).
#[cfg(target_os = "macos")]
mod macos {
    use std::sync::mpsc::{self, Receiver, Sender};

    use super::DictationIndicatorEvent;

    #[derive(Clone)]
    pub struct DictationIndicatorSender(Sender<DictationIndicatorEvent>);

    impl DictationIndicatorSender {
        pub fn send(&self, event: DictationIndicatorEvent) {
            let _ = self.0.send(event);
        }

        pub fn meter(&self, samples: &[f32]) {
            if let Some(event) = DictationIndicatorEvent::meter(samples) {
                self.send(event);
            }
        }
    }

    pub fn channel() -> (DictationIndicatorSender, Receiver<DictationIndicatorEvent>) {
        let (sender, receiver) = mpsc::channel();
        (DictationIndicatorSender(sender), receiver)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn meter_reports_rms_and_peak() {
            let (sender, receiver) = channel();
            sender.meter(&[-0.5, 0.5]);
            let DictationIndicatorEvent::Meter { average, peak } = receiver.recv().unwrap() else {
                panic!("expected meter event");
            };
            assert!((average - 0.5).abs() < f32::EPSILON);
            assert!((peak - 0.5).abs() < f32::EPSILON);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Hidden,
    Recording,
    Transcribing,
    Completed,
    Cancelled,
    Failed,
}

fn active_phase(capturing: bool, pending_jobs: usize) -> Option<Phase> {
    if capturing {
        Some(Phase::Recording)
    } else if pending_jobs > 0 {
        Some(Phase::Transcribing)
    } else {
        None
    }
}

impl Phase {
    fn visible_duration(self) -> Option<Duration> {
        match self {
            Self::Completed => Some(COMPLETED_VISIBLE_DURATION),
            Self::Cancelled => Some(Duration::ZERO),
            Self::Failed => Some(FAILED_VISIBLE_DURATION),
            _ => None,
        }
    }

    /// The look this phase shows; `None` keeps the last one as it fades.
    fn stage(self) -> Option<Stage> {
        match self {
            Self::Recording => Some(Stage::Recording),
            Self::Transcribing => Some(Stage::Processing),
            Self::Completed => Some(Stage::Done),
            Self::Failed => Some(Stage::Failed),
            Self::Hidden | Self::Cancelled => None,
        }
    }
}

/// Which indicator a frame draws on the HUD plate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Look {
    /// Dot scope's braided traces (the pill).
    Scope,
    /// The audio bar's level meter.
    Bars,
}

/// What a style puts on screen: the HUD plate with a look, and whether
/// the live text plate shows. Exactly one is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Surfaces {
    pub hud: Option<Look>,
    pub text: bool,
}

impl IndicatorStyle {
    /// The one indicator surface this style shows.
    pub fn surfaces(self) -> Surfaces {
        match self {
            Self::Pill => Surfaces {
                hud: Some(Look::Scope),
                text: false,
            },
            Self::AudioBar => Surfaces {
                hud: Some(Look::Bars),
                text: false,
            },
            Self::LiveText => Surfaces {
                hud: None,
                text: true,
            },
        }
    }
}

/// What Live text says while no words stream: the dictation's phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Label {
    Ready,
    LoadingModel,
    /// Bluetooth earbuds are switching to headset mode.
    ConnectingEarbuds,
    Recording,
    Transcribing,
    Done,
    Copied,
    Error,
}

impl Label {
    pub const fn text(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::LoadingModel => "Loading model",
            Self::ConnectingEarbuds => "Connecting earbuds mic…",
            Self::Recording => "Recording",
            Self::Transcribing => "Transcribing",
            Self::Done => "Done",
            Self::Copied => "Copied",
            Self::Error => "Error",
        }
    }

    /// Whether the label is an outcome, which clears by itself.
    pub const fn is_outcome(self) -> bool {
        matches!(self, Self::Done | Self::Copied | Self::Error)
    }
}

/// How long an outcome label shows.
pub const OUTCOME_LABEL_DURATION: Duration = Duration::from_millis(1200);

/// One frame for the HUD plate: its pose and every dot's light.
#[derive(Clone, Debug, PartialEq)]
pub struct DotFrame {
    pub look: Look,
    pub pose: Pose,
    pub tuning: Tuning,
    pub columns: usize,
    pub rows: usize,
    /// (brightness, freshness) per dot, row by row, as normalized u16s.
    pub dots: Vec<[u16; 2]>,
}

/// The middle of `rune_hud.wgsl`'s fade of dots near the field's edge
/// (1.4 to 4.6 pt in): a row or column nearer than this barely shows.
const FIELD_EDGE_FADE: f32 = 3.0;

/// How far in from the top and bottom the outermost row that shows sits.
fn field_edge(tuning: &Tuning) -> f32 {
    let outer = (tuning.height - (tuning.rows - 1.0) * tuning.spacing) / 2.0;
    if outer < FIELD_EDGE_FADE && tuning.rows > 2.0 {
        outer + tuning.spacing
    } else {
        outer
    }
}

/// The tuning that fills an island: the field as wide as the island less
/// its padding, recording and processing alike, at the same dot spacing.
/// The width snaps to whole columns, with the outer columns as far in from
/// the ends as the outermost rows that show are from the top and bottom,
/// so the island (which follows the field) is even all round.
fn fill_island(tuning: Tuning, island: Option<Island>) -> Tuning {
    let Some(island) = island else {
        return tuning;
    };
    let pitch = tuning.spacing;
    let edge = field_edge(&tuning);
    let columns = ((island.plate_width() - 2.0 * edge) / pitch).floor() + 1.0;
    let width = (columns - 1.0) * pitch + 2.0 * edge;
    Tuning {
        columns,
        width,
        processing_width: width,
        ..tuning
    }
}

/// The HUD's state: what the listener reported and Dot scope's simulation.
pub struct IndicatorModel {
    phase: Phase,
    capturing: bool,
    jobs: BTreeSet<u64>,
    phase_started: Instant,
    /// When the processing look began, for [`PROCESSING_MIN_DURATION`].
    working_since: Instant,
    /// The last job finished; done shows once processing has had its
    /// minimum.
    completion_pending: bool,
    scope: Scope,
    stage: Stage,
    /// The tuning as set, before [`fill_island`].
    tuning: Tuning,
    /// The notch island the HUD sits in, if any.
    island: Option<Island>,
    /// The microphone's loudness, dBFS.
    loudness: f32,
    /// HUD Lab's synthetic voice and how far into it Dot scope is.
    demo_voice: Option<f32>,
    /// Since an event, until a step finds everything settled out of sight.
    active: bool,
    style: IndicatorStyle,
    bars: Bars,
    /// A meter above the room's noise has arrived since the capture began.
    heard: bool,
    loading: bool,
    /// Earbuds are switching to headset mode; nothing is heard yet.
    earbuds: bool,
    copied: bool,
    /// The label now showing and since when, for [`Self::label`].
    label: Option<(Label, Instant)>,
}

impl IndicatorModel {
    pub fn new(tuning: Tuning, now: Instant) -> Self {
        Self {
            phase: Phase::Hidden,
            capturing: false,
            jobs: BTreeSet::new(),
            phase_started: now,
            working_since: now,
            completion_pending: false,
            scope: Scope::new(tuning),
            stage: Stage::Recording,
            tuning,
            island: None,
            loudness: SILENT,
            demo_voice: None,
            active: false,
            style: IndicatorStyle::default(),
            bars: Bars::new(),
            heard: false,
            loading: false,
            earbuds: false,
            copied: false,
            label: None,
        }
    }

    /// Picks the indicator the next dictation shows.
    pub fn set_style(&mut self, style: IndicatorStyle) {
        self.style = style;
    }

    /// The text Live text shows for the dictation's phase, `None` once an
    /// outcome has shown its time or nothing records.
    pub fn label(&self, now: Instant) -> Option<Label> {
        if self.style != IndicatorStyle::LiveText {
            return None;
        }
        let (label, since) = self.label?;
        if label.is_outcome() && now.saturating_duration_since(since) >= OUTCOME_LABEL_DURATION {
            return None;
        }
        Some(label)
    }

    /// The surfaces the picked style shows.
    pub fn surfaces(&self) -> Surfaces {
        self.style.surfaces()
    }

    fn derive_label(&self) -> Option<Label> {
        if self.phase == Phase::Failed {
            return Some(Label::Error);
        }
        if self.copied {
            return Some(Label::Copied);
        }
        match self.phase {
            Phase::Recording if self.earbuds => Some(Label::ConnectingEarbuds),
            Phase::Recording if self.loading => Some(Label::LoadingModel),
            Phase::Recording if !self.heard => Some(Label::Ready),
            Phase::Recording => Some(Label::Recording),
            Phase::Transcribing if self.completion_pending => Some(Label::Done),
            Phase::Transcribing => Some(Label::Transcribing),
            Phase::Completed => Some(Label::Done),
            Phase::Hidden | Phase::Cancelled | Phase::Failed => {
                self.loading.then_some(Label::LoadingModel)
            }
        }
    }

    fn refresh_label(&mut self, now: Instant) {
        let next = self.derive_label();
        if self.label.map(|(label, _)| label) != next {
            self.label = next.map(|label| (label, now));
        }
    }

    /// Whether the HUD wants frames: from an event until a step finds it
    /// settled out of sight.
    pub fn active(&self) -> bool {
        self.active && self.style.surfaces().hud.is_some()
    }

    /// Dot scope's tuning, as set.
    #[cfg(target_os = "macos")]
    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    /// Spreads Dot scope's field across the notch island it now sits in.
    #[cfg(any(target_os = "macos", test))]
    pub fn set_island(&mut self, island: Option<Island>) {
        if self.island != island {
            self.island = island;
            self.scope.tune(fill_island(self.tuning, island));
        }
    }

    pub fn handle(&mut self, event: DictationIndicatorEvent, now: Instant, reduce: bool) {
        self.active = true;
        match event {
            DictationIndicatorEvent::Reset => {
                self.capturing = false;
                self.jobs.clear();
                self.phase = Phase::Hidden;
                self.completion_pending = false;
                self.loudness = SILENT;
                self.demo_voice = None;
                self.heard = false;
                self.loading = false;
                self.earbuds = false;
                self.copied = false;
                self.bars.clear();
            }
            DictationIndicatorEvent::ConfigureDotScope(tuning) => {
                self.tuning = tuning;
                self.scope.tune(fill_island(tuning, self.island));
            }
            DictationIndicatorEvent::DemoVoice => {
                self.demo_voice = Some(0.0);
            }
            DictationIndicatorEvent::Started => {
                // HUD Lab and the playground send `DemoVoice` after this.
                self.demo_voice = None;
                self.capturing = true;
                self.phase = Phase::Recording;
                self.phase_started = now;
                self.completion_pending = false;
                self.loudness = SILENT;
                self.stage = Stage::Recording;
                self.scope.restart(reduce);
                self.heard = false;
                self.copied = false;
                self.bars.clear();
            }
            DictationIndicatorEvent::Meter { average, .. } => {
                self.loudness = dot_scope::loudness(average);
                self.heard |= self.capturing && self.loudness > HEARD;
            }
            DictationIndicatorEvent::Submitted { job_id } => {
                self.capturing = false;
                self.jobs.insert(job_id);
                self.show_pipeline(now);
            }
            DictationIndicatorEvent::Transcribing { job_id }
            | DictationIndicatorEvent::Processing { job_id } => {
                if self.jobs.contains(&job_id) {
                    self.show_pipeline(now);
                }
            }
            DictationIndicatorEvent::Discarded => {
                self.finish_capture(now, Phase::Hidden);
            }
            DictationIndicatorEvent::Cancelled => {
                self.finish_capture(now, Phase::Cancelled);
            }
            DictationIndicatorEvent::ModelLoading => self.loading = true,
            DictationIndicatorEvent::ModelReady => self.loading = false,
            DictationIndicatorEvent::EarbudsConnecting => self.earbuds = true,
            DictationIndicatorEvent::EarbudsReady => self.earbuds = false,
            DictationIndicatorEvent::Copied => self.copied = true,
            DictationIndicatorEvent::JobCompleted { job_id } => {
                self.finish_job(job_id, now, Phase::Completed);
            }
            DictationIndicatorEvent::JobCancelled { job_id } => {
                self.finish_job(job_id, now, Phase::Cancelled);
            }
            DictationIndicatorEvent::JobFailed { job_id } => {
                self.finish_job(job_id, now, Phase::Failed);
            }
            DictationIndicatorEvent::Failed => {
                self.finish_capture(now, Phase::Failed);
            }
        }
        self.refresh_label(now);
    }

    fn finish_capture(&mut self, now: Instant, empty_phase: Phase) {
        self.capturing = false;
        // A capture that ends while earbuds connect never gets EarbudsReady.
        self.earbuds = false;
        if !matches!(empty_phase, Phase::Hidden) {
            self.phase_started = now;
        }
        self.completion_pending = false;
        if self.jobs.is_empty() {
            self.phase = empty_phase;
        } else {
            self.show_pipeline(now);
        }
    }

    fn show_pipeline(&mut self, now: Instant) {
        let Some(phase) = active_phase(self.capturing, self.jobs.len()) else {
            return;
        };
        if phase == Phase::Transcribing && self.phase != Phase::Transcribing {
            self.working_since = now;
        }
        self.phase = phase;
        if phase == Phase::Recording {
            return;
        }
        self.phase_started = now;
        self.completion_pending = false;
    }

    fn finish_job(&mut self, job_id: u64, now: Instant, terminal: Phase) {
        if !self.jobs.remove(&job_id) {
            return;
        }
        if active_phase(self.capturing, self.jobs.len()).is_some() {
            self.show_pipeline(now);
            return;
        }
        if terminal == Phase::Completed {
            if self.phase != Phase::Transcribing {
                self.phase = Phase::Transcribing;
                self.working_since = now;
            }
            self.completion_pending = true;
        } else {
            self.phase = terminal;
            self.phase_started = now;
            self.completion_pending = false;
        }
    }

    /// Advances `dt` seconds to `now` and returns what to draw. Once
    /// everything has settled out of sight, [`Self::active`] turns false.
    pub fn step(&mut self, now: Instant, dt: f32, reduce: bool) -> DotFrame {
        let dt = dt.clamp(1.0 / 240.0, 1.0 / 20.0);
        if self.completion_pending
            && now.duration_since(self.working_since) >= PROCESSING_MIN_DURATION
        {
            self.phase = Phase::Completed;
            self.phase_started = now;
            self.completion_pending = false;
        }
        let elapsed = now.duration_since(self.phase_started);
        let terminal_finished = self
            .phase
            .visible_duration()
            .is_some_and(|duration| elapsed >= duration);
        let visible = self.phase != Phase::Hidden && !terminal_finished;
        if let Some(stage) = self.phase.stage() {
            self.stage = stage;
        }
        let loudness = match &mut self.demo_voice {
            Some(time) => {
                *time += dt;
                dot_scope::demo_loudness(*time)
            }
            None => self.loudness,
        };
        self.scope.advance(
            dt,
            dot_scope::Frame {
                stage: self.stage,
                visible,
                loudness,
                reduce_motion: reduce,
            },
        );
        self.active = visible || !self.scope.settled();
        let (columns, rows) = (self.scope.columns(), self.scope.rows());
        let (look, dots) = match self.style.surfaces().hud {
            Some(Look::Bars) => {
                let count = audio_bar::bar_count(columns);
                let levels = if self.stage == Stage::Processing {
                    let working = now.saturating_duration_since(self.working_since);
                    Bars::working(count, working.as_secs_f32())
                } else {
                    let level = if self.stage == Stage::Recording && visible {
                        self.scope.level()
                    } else {
                        0.0
                    };
                    self.bars.advance(dt, level);
                    self.bars.levels(count)
                };
                (Look::Bars, audio_bar::dots(&levels, columns, rows))
            }
            _ => (
                Look::Scope,
                self.scope
                    .dots()
                    .iter()
                    .map(|value| value.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16))
                    .collect(),
            ),
        };
        DotFrame {
            look,
            pose: self.scope.pose(),
            tuning: self.scope.tuning(),
            columns,
            rows,
            dots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_island_spreads_the_field_across_it_at_the_same_spacing() {
        let tuning = Tuning::default();
        assert_eq!(fill_island(tuning, None), tuning);
        let island = Island {
            notch: [185.0, 32.0],
            width: 241.0,
            top: -64.0,
        };
        let filled = fill_island(tuning, Some(island)).clamped();
        assert_eq!((filled.columns, filled.rows), (69.0, tuning.rows));
        assert_eq!(filled.width, filled.processing_width);
        // The outer columns sit as far in from the ends as the outermost
        // rows that show from the top and bottom (the faint outer row
        // doesn't), within the island less its padding.
        let side = (filled.width - (filled.columns - 1.0) * filled.spacing) / 2.0;
        assert!((side - field_edge(&filled)).abs() < 1e-3);
        assert!((field_edge(&filled) - 3.0).abs() < 1e-3);
        assert!(
            filled.width <= island.plate_width()
                && island.plate_width() - filled.width < filled.spacing
        );
    }

    #[test]
    fn meter_helper_reports_rms_and_peak() {
        let Some(DictationIndicatorEvent::Meter { average, peak }) =
            DictationIndicatorEvent::meter(&[-0.5, 0.5])
        else {
            panic!("expected meter event");
        };
        assert!((average - 0.5).abs() < f32::EPSILON);
        assert!((peak - 0.5).abs() < f32::EPSILON);
        assert!(DictationIndicatorEvent::meter(&[]).is_none());
    }

    fn at(start: Instant, seconds: f32) -> Instant {
        start + Duration::from_secs_f32(seconds)
    }

    fn run(model: &mut IndicatorModel, start: Instant, from: f32, seconds: f32) -> Vec<DotFrame> {
        (0..(seconds * 60.0) as usize)
            .map(|n| model.step(at(start, from + n as f32 / 60.0), 1.0 / 60.0, false))
            .collect()
    }

    fn lit(frame: &DotFrame) -> u32 {
        frame.dots.iter().map(|[power, _]| u32::from(*power)).sum()
    }

    /// A hot Linux microphone speaks near −18 dBFS RMS, which a fixed gain
    /// pins at full, so the HUD sat still: the auto level keeps it moving.
    #[test]
    fn a_hot_microphone_still_moves_the_scope() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        let mut levels = Vec::new();
        for n in 0..360 {
            let t = n as f32 / 60.0;
            let now = at(start, t);
            // Syllables at 4 Hz between −24 and −12 dBFS.
            let db = -18.0 + 6.0 * (t * 4.0 * std::f32::consts::TAU).sin();
            let rms = 10_f32.powf(db / 20.0);
            model.handle(
                DictationIndicatorEvent::Meter {
                    average: rms,
                    peak: (rms * 3.0).min(1.0),
                },
                now,
                false,
            );
            model.step(now, 1.0 / 60.0, false);
            if t > 2.0 {
                levels.push(model.scope.level());
            }
        }
        let low = levels.iter().copied().fold(f32::MAX, f32::min);
        let high = levels.iter().copied().fold(0.0, f32::max);
        assert!(high - low > 0.35, "the level swings {low}..{high}");
    }

    #[test]
    fn a_dictation_records_processes_finishes_and_settles() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        model.handle(DictationIndicatorEvent::DemoVoice, start, false);
        let frames = run(&mut model, start, 0.0, 1.0);
        let frame = frames.last().unwrap();
        assert_eq!(frame.dots.len(), frame.columns * frame.rows);
        assert!(frame.dots.iter().any(|[power, _]| *power > 30_000));
        assert!(frame.pose.opacity > 0.98 && frame.pose.cool < 0.01);

        // A fast transcription: the job finishes as soon as it is submitted.
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        model.handle(
            DictationIndicatorEvent::JobCompleted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        run(&mut model, start, 1.0, 0.15);
        assert_eq!(model.stage, Stage::Processing, "processing shows briefly");
        run(&mut model, start, 1.15, 0.1);
        assert_eq!(model.stage, Stage::Done);
        run(&mut model, start, 1.25, 2.0);
        assert!(!model.active(), "settled out of sight");
    }

    #[test]
    fn processing_that_ran_long_enough_finishes_at_once() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 1 },
            at(start, 0.5),
            false,
        );
        run(&mut model, start, 0.5, 0.5);
        model.handle(
            DictationIndicatorEvent::JobCompleted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        run(&mut model, start, 1.0, 1.0 / 60.0);
        assert_eq!(model.stage, Stage::Done);
    }

    #[test]
    fn a_failure_turns_coral_and_a_cancel_just_fades() {
        let start = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        run(&mut model, start, 0.0, 0.5);
        model.handle(DictationIndicatorEvent::Failed, at(start, 0.5), false);
        let failed = run(&mut model, start, 0.5, 0.5);
        assert!(failed.last().unwrap().pose.fail > 0.9);
        // The failure stays on screen for about four seconds, long enough to read.
        run(&mut model, start, 1.0, 1.5);
        assert!(model.active(), "still showing the failure");
        run(&mut model, start, 2.5, 3.0);
        assert!(!model.active());

        model.handle(DictationIndicatorEvent::Started, at(start, 6.0), false);
        run(&mut model, start, 6.0, 0.5);
        model.handle(DictationIndicatorEvent::Cancelled, at(start, 6.5), false);
        let cancelled = run(&mut model, start, 6.5, 1.5);
        assert!(cancelled.iter().all(|frame| frame.pose.fail < 0.01));
        assert!(lit(cancelled.last().unwrap()) == 0 || !model.active());
        assert!(!model.active());
    }

    /// A dictation as the listener reports it: voice at `loudness` dBFS
    /// for `seconds`, a frame every 1/60 s, then (optionally) submitted,
    /// transcribed and completed. Returns the frames.
    fn dictate(
        model: &mut IndicatorModel,
        start: Instant,
        seconds: f32,
        loudness: f32,
    ) -> Vec<DotFrame> {
        model.handle(DictationIndicatorEvent::Started, start, false);
        (0..(seconds * 60.0) as usize)
            .map(|n| {
                let now = at(start, n as f32 / 60.0);
                let rms = 10_f32.powf(loudness / 20.0);
                model.handle(
                    DictationIndicatorEvent::Meter {
                        average: rms,
                        peak: rms,
                    },
                    now,
                    false,
                );
                model.step(now, 1.0 / 60.0, false)
            })
            .collect()
    }

    fn model_with(style: IndicatorStyle, start: Instant) -> IndicatorModel {
        let mut model = IndicatorModel::new(Tuning::default(), start);
        model.set_style(style);
        model
    }

    #[test]
    fn each_style_shows_only_its_own_surface() {
        assert_eq!(
            IndicatorStyle::Pill.surfaces(),
            Surfaces {
                hud: Some(Look::Scope),
                text: false
            }
        );
        assert_eq!(
            IndicatorStyle::AudioBar.surfaces(),
            Surfaces {
                hud: Some(Look::Bars),
                text: false
            }
        );
        assert_eq!(
            IndicatorStyle::LiveText.surfaces(),
            Surfaces {
                hud: None,
                text: true
            }
        );
        // And the model draws what its style says: the pill and the bar
        // each draw their look on the HUD, Live text draws no HUD at all.
        let start = Instant::now();
        for (style, look) in [
            (IndicatorStyle::Pill, Some(Look::Scope)),
            (IndicatorStyle::AudioBar, Some(Look::Bars)),
            (IndicatorStyle::LiveText, None),
        ] {
            let mut model = model_with(style, start);
            let frames = dictate(&mut model, start, 0.5, -20.0);
            assert_eq!(
                model.active(),
                look.is_some(),
                "{style:?}: the HUD wants frames only when it shows"
            );
            assert_eq!(
                frames.last().map(|frame| frame.look),
                look.or(Some(Look::Scope)),
                "{style:?}"
            );
            assert_eq!(
                model.label(at(start, 0.5)).is_some(),
                style == IndicatorStyle::LiveText,
                "{style:?}: only Live text says a phase"
            );
        }
    }

    #[test]
    fn the_audio_bars_follow_the_meter() {
        let start = Instant::now();
        let mut model = model_with(IndicatorStyle::AudioBar, start);
        let loud = dictate(&mut model, start, 1.5, -12.0);
        assert_eq!(loud.last().unwrap().look, Look::Bars);
        let loud = lit(loud.last().unwrap());
        let mut model = model_with(IndicatorStyle::AudioBar, start);
        let quiet = dictate(&mut model, start, 1.5, -90.0);
        let quiet = lit(quiet.last().unwrap());
        assert!(loud > quiet * 2, "loud {loud} against quiet {quiet}");

        // The bars fall back when the voice stops.
        let mut model = model_with(IndicatorStyle::AudioBar, start);
        dictate(&mut model, start, 1.0, -12.0);
        let frames = (0..90)
            .map(|n| {
                let now = at(start, 1.0 + n as f32 / 60.0);
                model.handle(
                    DictationIndicatorEvent::Meter {
                        average: 0.0,
                        peak: 0.0,
                    },
                    now,
                    false,
                );
                model.step(now, 1.0 / 60.0, false)
            })
            .collect::<Vec<_>>();
        assert!(lit(frames.last().unwrap()) < lit(&frames[0]) / 2);
        assert!(lit(frames.last().unwrap()) < loud / 2);
    }

    #[test]
    fn the_audio_bar_follows_the_pills_phases() {
        let start = Instant::now();
        let mut model = model_with(IndicatorStyle::AudioBar, start);
        dictate(&mut model, start, 1.0, -20.0);
        assert_eq!(model.stage, Stage::Recording);
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        let working = run(&mut model, start, 1.0, 0.5);
        assert_eq!(model.stage, Stage::Processing);
        assert!(working.iter().all(|frame| frame.look == Look::Bars));
        model.handle(
            DictationIndicatorEvent::JobCompleted { job_id: 1 },
            at(start, 1.5),
            false,
        );
        run(&mut model, start, 1.5, 0.5);
        assert_eq!(model.stage, Stage::Done);
        run(&mut model, start, 2.0, 2.0);
        assert!(!model.active(), "settled out of sight");

        model.handle(DictationIndicatorEvent::Started, at(start, 5.0), false);
        run(&mut model, start, 5.0, 0.5);
        model.handle(DictationIndicatorEvent::Failed, at(start, 5.5), false);
        let failed = run(&mut model, start, 5.5, 0.5);
        assert_eq!(model.stage, Stage::Failed);
        assert!(failed.last().unwrap().pose.fail > 0.9);
    }

    #[test]
    fn earbuds_connecting_is_said_until_their_microphone_is_ready() {
        let start = Instant::now();
        let mut model = model_with(IndicatorStyle::LiveText, start);
        model.handle(DictationIndicatorEvent::Started, start, false);
        model.handle(DictationIndicatorEvent::EarbudsConnecting, start, false);
        assert_eq!(model.label(start), Some(Label::ConnectingEarbuds));
        model.handle(DictationIndicatorEvent::EarbudsReady, at(start, 1.0), false);
        assert_eq!(model.label(at(start, 1.0)), Some(Label::Ready));
        model.handle(
            DictationIndicatorEvent::EarbudsConnecting,
            at(start, 2.0),
            false,
        );
        model.handle(DictationIndicatorEvent::Reset, at(start, 2.1), false);
        assert_eq!(model.label(at(start, 2.1)), None, "a reset forgets it");
        // A cancel without EarbudsReady must not leave the next dictation stuck.
        model.handle(
            DictationIndicatorEvent::EarbudsConnecting,
            at(start, 3.0),
            false,
        );
        model.handle(DictationIndicatorEvent::Cancelled, at(start, 3.1), false);
        assert_ne!(model.label(at(start, 3.1)), Some(Label::ConnectingEarbuds));
        model.handle(DictationIndicatorEvent::Started, at(start, 3.3), false);
        assert_eq!(model.label(at(start, 3.3)), Some(Label::Ready));
    }

    #[test]
    fn live_text_says_the_phase_when_no_words_stream() {
        let start = Instant::now();
        let mut model = model_with(IndicatorStyle::LiveText, start);
        assert_eq!(model.label(start), None, "nothing before a dictation");
        model.handle(DictationIndicatorEvent::ModelLoading, start, false);
        assert_eq!(model.label(start), Some(Label::LoadingModel));
        model.handle(DictationIndicatorEvent::ModelReady, start, false);
        model.handle(DictationIndicatorEvent::Started, at(start, 0.1), false);
        assert_eq!(model.label(at(start, 0.1)), Some(Label::Ready));
        let quiet = |model: &mut IndicatorModel, t| {
            model.handle(
                DictationIndicatorEvent::Meter {
                    average: 1e-5,
                    peak: 1e-5,
                },
                at(start, t),
                false,
            );
        };
        quiet(&mut model, 0.2);
        assert_eq!(model.label(at(start, 0.2)), Some(Label::Ready));
        model.handle(
            DictationIndicatorEvent::Meter {
                average: 0.1,
                peak: 0.2,
            },
            at(start, 0.3),
            false,
        );
        assert_eq!(model.label(at(start, 0.3)), Some(Label::Recording));
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 1 },
            at(start, 1.0),
            false,
        );
        assert_eq!(model.label(at(start, 1.0)), Some(Label::Transcribing));
        model.handle(
            DictationIndicatorEvent::JobCompleted { job_id: 1 },
            at(start, 1.5),
            false,
        );
        assert_eq!(model.label(at(start, 1.5)), Some(Label::Done));
        assert_eq!(
            model.label(at(start, 1.5) + OUTCOME_LABEL_DURATION),
            None,
            "an outcome clears by itself"
        );

        // A copy instead of a typing, and a failure.
        model.handle(DictationIndicatorEvent::Started, at(start, 3.0), false);
        model.handle(
            DictationIndicatorEvent::Submitted { job_id: 2 },
            at(start, 3.5),
            false,
        );
        model.handle(DictationIndicatorEvent::Copied, at(start, 4.0), false);
        assert_eq!(model.label(at(start, 4.0)), Some(Label::Copied));
        model.handle(
            DictationIndicatorEvent::JobFailed { job_id: 2 },
            at(start, 4.1),
            false,
        );
        assert_eq!(model.label(at(start, 4.1)), Some(Label::Error));

        // A cancel shows nothing.
        model.handle(DictationIndicatorEvent::Started, at(start, 6.0), false);
        model.handle(DictationIndicatorEvent::Cancelled, at(start, 6.2), false);
        assert_eq!(model.label(at(start, 6.2)), None);
    }

    #[test]
    fn pending_completion_cannot_replace_a_new_recording_phase() {
        assert_eq!(active_phase(true, 0), Some(Phase::Recording));
        assert_eq!(active_phase(true, 2), Some(Phase::Recording));
        assert_eq!(active_phase(false, 2), Some(Phase::Transcribing));
        assert_eq!(active_phase(false, 0), None);
    }
}
