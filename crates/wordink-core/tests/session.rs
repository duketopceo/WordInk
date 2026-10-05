use wordink_core::providers::{Capabilities, HostProvider, Provider};
use wordink_core::{Effect, ErrorCode, Event, HostResult, Mode, Session, SessionConfig, State};

const MIC_RATE: u32 = 48_000;

/// A session with a batch host provider at 16 kHz, so the utterance arrives
/// whole on release, as with any batch provider.
fn session(mode: Mode) -> Session {
    let provider = Provider::Host(HostProvider::new(
        "test",
        Capabilities {
            streaming: false,
            sample_rate: 16_000,
        },
    ));
    Session::new(SessionConfig::new(mode, provider))
}

fn final_result(text: &str) -> Event {
    Event::HostProviderResult(HostResult::Final { text: text.into() })
}

/// A 440 Hz sine of `ms` milliseconds at the mic rate.
fn tone(ms: u32, amplitude: f32) -> Vec<f32> {
    let n = (MIC_RATE * ms / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / MIC_RATE as f32;
            amplitude * (2.0 * core::f32::consts::PI * 440.0 * t).sin()
        })
        .collect()
}

/// Pushes audio in AudioWorklet-sized chunks (128 frames) and collects effects.
fn push(s: &mut Session, samples: &[f32]) -> Vec<Effect> {
    samples.chunks(128).flat_map(|c| s.push_audio(c)).collect()
}

fn levels(effects: &[Effect]) -> Vec<f32> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Level { rms } => Some(*rms),
            _ => None,
        })
        .collect()
}

fn has_provider_effect(effects: &[Effect]) -> bool {
    effects.iter().any(|e| {
        !matches!(
            e,
            Effect::State(_) | Effect::RequestMic | Effect::StopMic | Effect::Level { .. }
        )
    })
}

/// Drives a full dictation using `stop` as the stop input, returning every
/// effect in order.
fn full_run(mode: Mode, stop: Event) -> Vec<Effect> {
    let mut s = session(mode);
    let mut all = Vec::new();
    all.extend(s.handle(Event::Press));
    all.extend(s.handle(Event::MicGranted {
        sample_rate: MIC_RATE,
    }));
    all.extend(push(&mut s, &tone(500, 0.5)));
    all.extend(s.handle(stop));
    all.extend(s.handle(final_result("hello world")));
    assert_eq!(s.state(), State::Idle);
    all
}

#[test]
fn push_to_talk_happy_path() {
    let mut s = session(Mode::PushToTalk);
    assert_eq!(s.state(), State::Idle);

    assert_eq!(
        s.handle(Event::Press),
        vec![Effect::State(State::RequestingMic), Effect::RequestMic]
    );
    assert_eq!(
        s.handle(Event::MicGranted {
            sample_rate: MIC_RATE
        }),
        vec![Effect::State(State::Listening)]
    );

    let audio = push(&mut s, &tone(500, 0.5));
    assert!(!levels(&audio).is_empty());

    let released = s.handle(Event::Release);
    assert_eq!(released[0], Effect::StopMic);
    assert_eq!(released[1], Effect::State(State::Transcribing));
    assert_eq!(
        released[2],
        Effect::HostProviderStart {
            id: "test".into(),
            sample_rate: 16_000,
            hint: None
        }
    );
    match &released[3] {
        Effect::HostProviderAudio { samples, .. } => {
            // 500 ms at 16 kHz.
            assert!((samples.len() as i64 - 8_000).abs() <= 1);
            assert!(samples.iter().any(|&x| x.unsigned_abs() > 10_000));
        }
        other => panic!("expected HostProviderAudio, got {other:?}"),
    }
    assert_eq!(
        released[4],
        Effect::HostProviderFinish { id: "test".into() }
    );
    assert_eq!(released.len(), 5);
    assert_eq!(s.state(), State::Transcribing);

    assert_eq!(
        s.handle(final_result("hello world")),
        vec![
            Effect::Final {
                text: "hello world".into()
            },
            Effect::State(State::Idle)
        ]
    );
    assert_eq!(s.state(), State::Idle);
}

#[test]
fn level_events_arrive_at_about_20_hz() {
    let mut s = session(Mode::PushToTalk);
    s.handle(Event::Press);
    s.handle(Event::MicGranted {
        sample_rate: MIC_RATE,
    });
    let lv = levels(&push(&mut s, &tone(1000, 0.5)));
    assert_eq!(lv.len(), 20);
    // RMS of a sine with amplitude A is A / sqrt(2).
    for rms in lv {
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.01, "rms {rms}");
    }
}

#[test]
fn toggle_mode_matches_push_to_talk() {
    let mut s = session(Mode::Toggle);
    s.handle(Event::Press);
    s.handle(Event::MicGranted {
        sample_rate: MIC_RATE,
    });
    // Releasing the button does nothing in toggle mode.
    assert!(s.handle(Event::Release).is_empty());
    assert_eq!(s.state(), State::Listening);

    let ptt = full_run(Mode::PushToTalk, Event::Release);
    let toggle = full_run(Mode::Toggle, Event::Press);
    assert_eq!(ptt, toggle);
}

#[test]
fn mic_denied_is_an_error_and_press_retries() {
    let mut s = session(Mode::PushToTalk);
    s.handle(Event::Press);
    assert_eq!(
        s.handle(Event::MicDenied),
        vec![Effect::State(State::Error(ErrorCode::MicDenied))]
    );
    assert_eq!(s.state(), State::Error(ErrorCode::MicDenied));

    assert_eq!(
        s.handle(Event::Press),
        vec![
            Effect::State(State::Idle),
            Effect::State(State::RequestingMic),
            Effect::RequestMic
        ]
    );
}

#[test]
fn under_300_ms_is_no_speech_without_provider_effect() {
    let mut s = session(Mode::PushToTalk);
    s.handle(Event::Press);
    s.handle(Event::MicGranted {
        sample_rate: MIC_RATE,
    });
    push(&mut s, &tone(250, 0.5));
    let effects = s.handle(Event::Release);
    assert!(!has_provider_effect(&effects));
    assert_eq!(
        effects,
        vec![
            Effect::StopMic,
            Effect::State(State::Error(ErrorCode::NoSpeech))
        ]
    );
    assert_eq!(s.state(), State::Error(ErrorCode::NoSpeech));
}

#[test]
fn silent_audio_is_no_speech_without_provider_effect() {
    let mut s = session(Mode::PushToTalk);
    s.handle(Event::Press);
    s.handle(Event::MicGranted {
        sample_rate: MIC_RATE,
    });
    push(&mut s, &vec![0.0; MIC_RATE as usize]);
    let effects = s.handle(Event::Release);
    assert!(!has_provider_effect(&effects));
    assert_eq!(s.state(), State::Error(ErrorCode::NoSpeech));
}
