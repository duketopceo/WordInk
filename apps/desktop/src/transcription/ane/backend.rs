//! The Neural Engine model as the engine's local transcriber: it serves a
//! dictation from the resident worker when the worker is ready, and from the
//! GPU Parakeet v3 otherwise, so no dictation is ever lost to the engine.
//!
//! The GPU model is loaded lazily, the first time something needs it, and is
//! dropped with the transcriber when the engine unloads an idle model.

use std::path::PathBuf;
use std::time::Instant;

use color_eyre::eyre::{Result, bail, eyre};

use super::probe::{self, Verdict};
use super::worker::{Config, Outcome, Supervisor};
use crate::app_settings::TranscriptionDevice;
use crate::transcription::engine::ServedBy;
use crate::transcription::local_transcriber::{GgufTranscriber, Served};
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection, validate};

/// The worker side of a dictation: the text, or why not.
pub(crate) trait WorkerLane {
    fn try_transcribe(&self, samples: &[f32]) -> Outcome;
}

impl WorkerLane for Supervisor {
    fn try_transcribe(&self, samples: &[f32]) -> Outcome {
        Supervisor::try_transcribe(self, samples)
    }
}

/// One dictation: the worker when it answers, else `gpu`, with the engine
/// that served it and that attempt's own time. A worker that is not ready, or
/// whose attempt fails, hands the same dictation to `gpu` once.
pub(crate) fn serve(
    worker: &dyn WorkerLane,
    gpu: &mut dyn FnMut(&[f32]) -> Result<String>,
    samples: &[f32],
) -> Result<Served> {
    let started = Instant::now();
    match worker.try_transcribe(samples) {
        Outcome::Text { text, .. } => {
            return Ok(Served {
                text,
                by: ServedBy {
                    model: TranscriptionModelId::ParakeetV3Ane,
                    inference_ms: started.elapsed().as_millis() as u64,
                },
            });
        }
        Outcome::NotReady => {}
        Outcome::Failed(why) => {
            tracing::warn!(%why, "the Neural Engine worker failed; transcribing on the GPU model");
        }
    }
    // Only this attempt is the GPU model's time.
    let started = Instant::now();
    let text = gpu(samples)?;
    Ok(Served {
        text,
        by: ServedBy {
            model: TranscriptionModelId::ParakeetV3,
            inference_ms: started.elapsed().as_millis() as u64,
        },
    })
}

pub(crate) struct AneTranscriber {
    selection: TranscriptionSelection,
    /// What answers while the worker cannot.
    gpu_selection: TranscriptionSelection,
    gpu: Option<GgufTranscriber>,
    supervisor: Supervisor,
}

impl AneTranscriber {
    /// Starts the worker in the background and returns at once; the model is
    /// usable immediately, through the GPU model until the worker is ready.
    pub(crate) fn load(selection: &TranscriptionSelection) -> Result<Self> {
        validate(selection)?;
        let probe = probe::current();
        let (Verdict::Available, Some(runtime)) = (probe.verdict, probe.runtime) else {
            bail!("Parakeet v3 (Neural Engine) is unavailable on this computer");
        };
        let supervisor = Supervisor::start(Config::new(runtime, run_dir()?))
            .map_err(|error| eyre!("could not start the Neural Engine worker: {error}"))?;
        tracing::info!("started the Neural Engine worker");
        Ok(Self::with_supervisor(selection, supervisor))
    }

    fn with_supervisor(selection: &TranscriptionSelection, supervisor: Supervisor) -> Self {
        Self {
            gpu_selection: TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV3,
                language: selection.language.clone(),
            },
            selection: selection.clone(),
            gpu: None,
            supervisor,
        }
    }

    pub(crate) fn selection(&self) -> &TranscriptionSelection {
        &self.selection
    }

    /// The Neural Engine model has no device setting of its own.
    pub(crate) fn preference(&self) -> TranscriptionDevice {
        crate::transcription::local_transcriber::device()
    }

    /// The GPU model, loaded now if it is not yet.
    pub(crate) fn gpu(&mut self) -> Result<&mut GgufTranscriber> {
        Self::session(&mut self.gpu, &self.gpu_selection)
    }

    fn session<'a>(
        slot: &'a mut Option<GgufTranscriber>,
        selection: &TranscriptionSelection,
    ) -> Result<&'a mut GgufTranscriber> {
        if slot.is_none() {
            *slot = Some(GgufTranscriber::load(selection)?);
        }
        slot.as_mut()
            .ok_or_else(|| eyre!("the GPU model did not load"))
    }

    /// A complete 16 kHz clip.
    pub(crate) fn serve_clip(&mut self, samples: &[f32]) -> Result<Served> {
        let Self {
            supervisor,
            gpu,
            gpu_selection,
            ..
        } = self;
        serve(
            supervisor,
            &mut |samples| Self::session(gpu, gpu_selection)?.transcribe(samples),
            samples,
        )
    }
}

/// A private directory for the worker's audio and scratch: the user's
/// runtime directory when there is one, else a folder beside WordInk's data.
fn run_dir() -> Result<PathBuf> {
    let parent = match dirs::runtime_dir() {
        Some(dir) => dir,
        None => crate::app_paths::support_dir()?,
    };
    Ok(parent.join("wordink-ane"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct FakeWorker {
        outcomes: RefCell<Vec<Outcome>>,
        calls: Cell<usize>,
    }

    impl FakeWorker {
        fn new(outcomes: Vec<Outcome>) -> Self {
            Self {
                outcomes: RefCell::new(outcomes),
                calls: Cell::new(0),
            }
        }
    }

    impl WorkerLane for FakeWorker {
        fn try_transcribe(&self, _: &[f32]) -> Outcome {
            self.calls.set(self.calls.get() + 1);
            self.outcomes.borrow_mut().remove(0)
        }
    }

    fn text(value: &str) -> Outcome {
        Outcome::Text {
            text: value.to_string(),
            pipeline_ms: Some(500.0),
        }
    }

    fn gpu_says<'a>(
        value: &'a str,
        used: &'a Cell<usize>,
    ) -> impl FnMut(&[f32]) -> Result<String> + 'a {
        move |_| {
            used.set(used.get() + 1);
            Ok(value.to_string())
        }
    }

    #[test]
    fn a_ready_worker_serves_the_dictation_and_the_gpu_is_not_touched() {
        let worker = FakeWorker::new(vec![text("from the engine")]);
        let used = Cell::new(0);
        let served = serve(&worker, &mut gpu_says("from the gpu", &used), &[0.0]).unwrap();
        assert_eq!(served.text, "from the engine");
        assert_eq!(served.by.model, TranscriptionModelId::ParakeetV3Ane);
        assert_eq!(used.get(), 0);
    }

    #[test]
    fn a_worker_still_starting_hands_the_dictation_to_the_gpu_and_the_next_uses_the_worker() {
        let worker = FakeWorker::new(vec![Outcome::NotReady, text("second")]);
        let used = Cell::new(0);
        let first = serve(&worker, &mut gpu_says("first", &used), &[0.0]).unwrap();
        assert_eq!(
            (first.text.as_str(), first.by.model),
            ("first", TranscriptionModelId::ParakeetV3)
        );
        let second = serve(&worker, &mut gpu_says("unused", &used), &[0.0]).unwrap();
        assert_eq!(
            (second.text.as_str(), second.by.model),
            ("second", TranscriptionModelId::ParakeetV3Ane)
        );
        assert_eq!(used.get(), 1);
    }

    #[test]
    fn a_failed_attempt_is_retried_once_on_the_gpu_within_the_same_dictation() {
        let worker = FakeWorker::new(vec![Outcome::Failed("timed out".into())]);
        let used = Cell::new(0);
        let served = serve(&worker, &mut gpu_says("rescued", &used), &[0.0]).unwrap();
        assert_eq!(served.text, "rescued");
        assert_eq!(served.by.model, TranscriptionModelId::ParakeetV3);
        assert_eq!((worker.calls.get(), used.get()), (1, 1));
    }

    #[test]
    fn a_fallback_that_cannot_run_reports_its_own_error_once() {
        let worker = FakeWorker::new(vec![Outcome::NotReady]);
        let error = serve(
            &worker,
            &mut |_| Err(eyre!("No speech model installed.")),
            &[0.0],
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "No speech model installed.");
    }

    #[test]
    fn only_the_serving_attempt_is_timed() {
        let worker = FakeWorker::new(vec![Outcome::Failed("slow".into())]);
        let served = serve(
            &worker,
            &mut |_| {
                std::thread::sleep(std::time::Duration::from_millis(30));
                Ok("x".to_string())
            },
            &[0.0],
        )
        .unwrap();
        assert!(served.by.inference_ms >= 30 && served.by.inference_ms < 5_000);
    }

    const STUB: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ane/stub_cli.py"
    );

    fn stub_transcriber(
        name: &str,
    ) -> (
        crate::transcription::local_transcriber::LocalTranscriber,
        PathBuf,
    ) {
        let run_dir =
            std::env::temp_dir().join(format!("wordink-ane-backend-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&run_dir);
        let config = Config::new(
            super::super::probe::Runtime {
                interpreter: PathBuf::from("python3"),
                cli_script: PathBuf::from(STUB),
                cache_root: None,
            },
            run_dir.clone(),
        );
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::ParakeetV3Ane,
            language: "en".into(),
        };
        let supervisor = Supervisor::start(config).unwrap();
        // Ready first, so no test reaches for the GPU model.
        let started = Instant::now();
        while supervisor.state() != super::super::worker::State::Ready {
            assert!(started.elapsed() < std::time::Duration::from_secs(30));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let ane = AneTranscriber::with_supervisor(&selection, supervisor);
        (
            crate::transcription::local_transcriber::LocalTranscriber::from_ane(ane),
            run_dir,
        )
    }

    #[test]
    fn the_neural_engine_model_never_streams_and_reports_its_own_selection() {
        let (model, run_dir) = stub_transcriber("streams");
        assert!(model.stream_options().is_none());
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::ParakeetV3Ane,
            language: "en".into(),
        };
        assert!(model.matches_selection(&selection));
        assert_eq!(model.gguf_selection(), Some(&selection));
        assert_eq!(model.device_label(), "Apple Neural Engine");
        drop(model);
        let _ = std::fs::remove_dir_all(run_dir);
    }

    #[test]
    fn the_answering_model_travels_from_the_worker_to_the_engine_once() {
        let (mut model, run_dir) = stub_transcriber("served");
        let served = model.transcribe_clip_served(vec![0.0; 16_000]).unwrap();
        assert_eq!(served.by.model, TranscriptionModelId::ParakeetV3Ane);
        let taken = model.take_served().unwrap();
        assert_eq!(taken.model, TranscriptionModelId::ParakeetV3Ane);
        assert!(model.take_served().is_none(), "taken once");
        drop(model);
        let _ = std::fs::remove_dir_all(run_dir);
    }

    #[test]
    fn a_stream_request_on_the_neural_engine_model_is_refused_not_attempted() {
        let (mut model, run_dir) = stub_transcriber("stream-refused");
        let options = transcribe_cpp::StreamOptions::default();
        let result = model.with_stream(&options, &transcribe_cpp::CancelToken::new(), |_| ());
        assert!(matches!(result, Err(transcribe_cpp::Error::Unsupported(_))));
        drop(model);
        let _ = std::fs::remove_dir_all(run_dir);
    }

    #[test]
    fn a_dictation_takes_one_worker_attempt_and_a_gpu_retry_inside_the_job_deadline() {
        // The engine's job deadline is 180 s plus eight per audio second; the
        // worker's own timeout must leave room for the GPU retry after it.
        let config = Config::new(
            super::super::probe::Runtime {
                interpreter: PathBuf::new(),
                cli_script: PathBuf::new(),
                cache_root: None,
            },
            PathBuf::new(),
        );
        for clip_seconds in [1u64, 15, 60, 300, 600] {
            let worker = config.request_base
                + config.request_per_clip_second * u32::try_from(clip_seconds).unwrap();
            let job = std::time::Duration::from_secs(180 + 8 * clip_seconds);
            assert!(
                worker * 2 <= job,
                "{clip_seconds}s: worker {worker:?}, job {job:?}"
            );
        }
    }
}
