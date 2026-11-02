//! Recovery of failed dictations (R11) and re-transcribing a retained clip
//! with another engine (R26).
//!
//! A cloud dictation that fails keeps its clip in `failed-dictations`, named
//! for the time and the engine that ran it (`cloud::clip_name`). History lists
//! those clips; this is what listing, retrying and deleting them does. A
//! retry sends the stored clip to the engine the person picks, never
//! inserts anything, and on success records the text in History as
//! `Recovered` (so Usage counts what it cost) and deletes the clip.
//!
//! Audio recorded for a local model has never left the machine. Sending it
//! to a cloud provider needs the person's explicit confirmation.

use std::fs;
use std::path::{Path, PathBuf};

use crate::history::{History, HistoryDraft, HistoryKind, UsageRecord};
use crate::transcription::cloud::{self, Provider};

/// A kept clip of a dictation that failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedClip {
    pub path: PathBuf,
    pub timestamp_ms: u64,
    /// The engine that ran it: a provider id, or `local`.
    pub origin: String,
}

impl FailedClip {
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The name of the engine that failed, for the list.
    pub fn origin_label(&self) -> String {
        match Provider::from_id(&self.origin) {
            Some(provider) => provider.name().to_owned(),
            None if self.origin == "local" => "Local model".to_owned(),
            None => self.origin.clone(),
        }
    }
}

/// The kept clips in `dir`, newest first. A file that isn't named like one
/// is not a clip and is left alone.
pub fn list(dir: &Path) -> Vec<FailedClip> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut clips: Vec<FailedClip> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let (timestamp_ms, origin) = cloud::parse_clip_name(path.file_name()?.to_str()?)?;
            Some(FailedClip {
                path,
                timestamp_ms,
                origin,
            })
        })
        .collect();
    clips.sort_by(|a, b| {
        b.timestamp_ms
            .cmp(&a.timestamp_ms)
            .then_with(|| b.path.cmp(&a.path))
    });
    clips
}

/// Deletes the clip named `name` from `dir`. Only a name `list` would show
/// is deleted, so a path can't reach outside the directory.
pub fn delete(dir: &Path, name: &str) -> std::io::Result<()> {
    if cloud::parse_clip_name(name).is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a kept clip",
        ));
    }
    match fs::remove_file(dir.join(name)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The samples of a stored clip (16 kHz mono 16-bit WAV, as `cloud` wrote it).
pub fn read_samples(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| format!("could not read the clip: {error}"))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 || spec.bits_per_sample != 16 {
        return Err("the kept clip is not 16 kHz mono audio".into());
    }
    reader
        .samples::<i16>()
        .map(|sample| {
            sample
                .map(|value| f32::from(value) / f32::from(i16::MAX))
                .map_err(|error| format!("could not read the clip: {error}"))
        })
        .collect()
}

/// The engine a retry sends the clip to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub provider: Provider,
    pub model: String,
    pub gateway_url: Option<String>,
}

/// Whether sending a clip that `origin` ran to a cloud provider must be
/// confirmed: yes when it was recorded for a local model. Every target is a
/// cloud provider.
pub fn needs_confirmation(origin: &str) -> bool {
    origin == "local"
}

#[derive(Debug, Eq, PartialEq)]
pub enum RetryError {
    /// The clip was recorded for a local model and the person has not said
    /// it may go to a cloud provider. Nothing was sent.
    NeedsConfirmation,
    Failed(String),
}

pub struct Retried {
    pub text: String,
    pub usage: UsageRecord,
    /// The History entry the text was recorded as, when History is on.
    #[allow(dead_code)] // read by tests; kept for a future "show in History" link
    pub entry_id: Option<u64>,
}

/// Retries `clip` on `target` with `run` doing the transcription. Records
/// success in `history` and deletes the clip; a failure leaves the clip.
pub fn retry_with(
    dir: &Path,
    clip: &FailedClip,
    target: &Target,
    confirmed: bool,
    history: Option<&History>,
    run: impl FnOnce(&Target, &[f32]) -> Result<(String, UsageRecord), String>,
) -> Result<Retried, RetryError> {
    if needs_confirmation(&clip.origin) && !confirmed {
        return Err(RetryError::NeedsConfirmation);
    }
    let samples = read_samples(&clip.path).map_err(RetryError::Failed)?;
    let (text, usage) = run(target, &samples).map_err(RetryError::Failed)?;
    let entry_id = history.and_then(|history| {
        history
            .record(HistoryDraft {
                usage: Some(usage.clone()),
                kind: HistoryKind::Recovered,
                raw_text: text.clone(),
                final_text: text.clone(),
                application: None,
                processing: None,
                audio_ms: (usage.audio_seconds * 1000.0) as u64,
                inference_ms: 0,
                total_ms: 0,
            })
            .ok()
            .flatten()
    });
    let _ = delete(dir, &clip.file_name());
    Ok(Retried {
        text,
        usage,
        entry_id,
    })
}

/// [`retry_with`] on the real engines. Blocks for the request, so run it on
/// a worker.
pub fn retry(
    dir: &Path,
    clip: &FailedClip,
    target: &Target,
    confirmed: bool,
    history: Option<&History>,
) -> Result<Retried, RetryError> {
    retry_with(dir, clip, target, confirmed, history, |target, samples| {
        let language =
            cloud::language_hint(&crate::app_settings::transcription_selection().1.language);
        cloud::run_with(
            target.provider,
            &target.model,
            target.gateway_url.as_deref(),
            samples,
            language.as_deref(),
            false,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryRetention, HistoryStore};
    use std::sync::Mutex;

    fn temp(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("wordink-recover-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn speech() -> Vec<f32> {
        (0..16_000)
            .map(|i| ((i as f32) * 0.05).sin() * 0.3)
            .collect()
    }

    /// A clip as `cloud` keeps one: 0600 in a failed-dictations directory.
    fn store_clip(dir: &Path, ms: u64, origin: &str, samples: &[f32]) -> FailedClip {
        let path = dir.join(cloud::clip_name(ms, origin));
        fs::write(&path, cloud::wav_bytes(samples)).unwrap();
        FailedClip {
            path,
            timestamp_ms: ms,
            origin: origin.into(),
        }
    }

    fn usage(provider: &str) -> UsageRecord {
        UsageRecord {
            provider: provider.into(),
            model: "m".into(),
            audio_seconds: 1.0,
            words: 3,
            cost_usd: 0.001,
        }
    }

    fn target(provider: Provider) -> Target {
        Target {
            provider,
            model: provider.default_model().into(),
            gateway_url: None,
        }
    }

    #[test]
    fn failed_dictations_are_listed_newest_first_with_the_engine_that_failed() {
        let dir = temp("list");
        store_clip(&dir, 100, "groq", &speech());
        store_clip(&dir, 300, "deepgram", &speech());
        store_clip(&dir, 200, "local", &speech());
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        fs::write(dir.join("failed-5.wav"), b"RIFF").unwrap();
        let clips = list(&dir);
        let seen: Vec<_> = clips
            .iter()
            .map(|clip| (clip.timestamp_ms, clip.origin.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (300, "deepgram"),
                (200, "local"),
                (100, "groq"),
                (5, "groq")
            ]
        );
        assert_eq!(clips[0].origin_label(), "Deepgram");
        assert_eq!(clips[1].origin_label(), "Local model");
        assert!(list(&dir.join("missing")).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_retry_sends_the_stored_clip_not_a_new_recording() {
        let dir = temp("clip");
        let samples = speech();
        let clip = store_clip(&dir, 1, "groq", &samples);
        let sent = Mutex::new(None);
        let retried = retry_with(
            &dir,
            &clip,
            &target(Provider::OpenAi),
            false,
            None,
            |target, heard| {
                *sent.lock().unwrap() = Some((target.clone(), heard.to_vec()));
                Ok(("recovered words".into(), usage("openai")))
            },
        )
        .unwrap();
        assert_eq!(retried.text, "recovered words");
        let (to, heard) = sent.into_inner().unwrap().unwrap();
        assert_eq!(to.provider, Provider::OpenAi);
        assert_eq!(heard.len(), samples.len());
        // 16-bit quantisation is all that differs from what was recorded.
        assert!(
            heard
                .iter()
                .zip(&samples)
                .all(|(a, b)| (a - b).abs() < 1.0 / 16_000.0)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_local_clip_goes_to_a_cloud_provider_only_after_confirmation() {
        let dir = temp("confirm");
        let clip = store_clip(&dir, 1, "local", &speech());
        let sent = Mutex::new(0);
        let run = |_: &Target, _: &[f32]| {
            *sent.lock().unwrap() += 1;
            Ok(("text".to_string(), usage("groq")))
        };
        let refused = retry_with(&dir, &clip, &target(Provider::Groq), false, None, run);
        assert_eq!(refused.err(), Some(RetryError::NeedsConfirmation));
        assert_eq!(
            *sent.lock().unwrap(),
            0,
            "nothing was sent before the person said yes"
        );
        assert!(clip.path.exists(), "the clip stays");
        retry_with(&dir, &clip, &target(Provider::Groq), true, None, run).unwrap();
        assert_eq!(*sent.lock().unwrap(), 1);
        assert!(
            needs_confirmation("local")
                && !needs_confirmation("groq")
                && !needs_confirmation("openrouter")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cloud_clip_retries_on_another_cloud_engine_without_asking() {
        let dir = temp("another");
        let clip = store_clip(&dir, 1, "groq", &speech());
        let retried = retry_with(
            &dir,
            &clip,
            &target(Provider::Deepgram),
            false,
            None,
            |_, _| Ok(("ok".into(), usage("deepgram"))),
        );
        assert!(retried.is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn success_records_the_text_as_recovered_and_deletes_the_clip_but_failure_keeps_it() {
        let dir = temp("outcome");
        let history = History::new(HistoryStore::open(
            dir.join("history.json"),
            HistoryRetention::Week,
            crate::history::now_ms(),
        ));
        let clip = store_clip(&dir, 1, "groq", &speech());
        let failed = retry_with(
            &dir,
            &clip,
            &target(Provider::OpenAi),
            false,
            Some(&history),
            |_, _| Err("OpenAI key rejected".into()),
        );
        assert_eq!(
            failed.err(),
            Some(RetryError::Failed("OpenAI key rejected".into()))
        );
        assert!(clip.path.exists(), "a failed retry keeps the clip");
        assert!(history.search("").is_empty(), "and records nothing");

        let retried = retry_with(
            &dir,
            &clip,
            &target(Provider::OpenAi),
            false,
            Some(&history),
            |_, _| Ok(("the words".into(), usage("openai"))),
        )
        .unwrap();
        assert!(!clip.path.exists(), "a recovered clip is deleted");
        let entries = history.search("");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, HistoryKind::Recovered);
        assert_eq!(entries[0].final_text, "the words");
        assert_eq!(entries[0].usage.as_ref().unwrap().provider, "openai");
        assert_eq!(retried.entry_id, Some(entries[0].id));
        // Nothing was inserted anywhere: this module has no paste path.
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_takes_only_a_kept_clips_name() {
        let dir = temp("delete");
        let clip = store_clip(&dir, 1, "groq", &speech());
        let outside = dir
            .parent()
            .unwrap()
            .join(format!("failed-9-groq-{}.wav", std::process::id()));
        fs::write(&outside, b"x").unwrap();
        assert!(delete(&dir, "../notes.txt").is_err());
        assert!(
            delete(
                &dir,
                &format!("../{}", outside.file_name().unwrap().to_string_lossy())
            )
            .is_err()
        );
        assert!(outside.exists());
        delete(&dir, &clip.file_name()).unwrap();
        assert!(!clip.path.exists());
        delete(&dir, &clip.file_name()).unwrap();
        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clip_that_is_not_the_recorders_format_is_refused() {
        let dir = temp("format");
        let path = dir.join("failed-1-groq.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.finalize().unwrap();
        assert!(read_samples(&path).unwrap_err().contains("16 kHz mono"));
        assert!(read_samples(&dir.join("absent.wav")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
