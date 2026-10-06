//! Provider protocol tests (U3). No network: provider replies come from
//! recorded fixtures in `tests/fixtures/`, which a manual live check can be
//! compared against.

use serde_json::Value;
use wordink_core::providers::{
    Capabilities, Deepgram, Groq, HostProvider, OpenAi, Provider, WS_CLOSE_STALL,
};
use wordink_core::{
    Effect, ErrorCode, Event, HostResult, HttpBody, HttpRequest, Mode, PartValue, Session,
    SessionConfig, State, WsData,
};

const MIC_RATE: u32 = 48_000;
const HINT: &str = "WordInk, Omarchy";

// ---------------------------------------------------------------- helpers

fn tone(ms: u32, amplitude: f32) -> Vec<f32> {
    let n = (MIC_RATE * ms / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / MIC_RATE as f32;
            amplitude * (2.0 * core::f32::consts::PI * 440.0 * t).sin()
        })
        .collect()
}

fn session(provider: Provider) -> Session {
    let mut config = SessionConfig::new(Mode::PushToTalk, provider);
    config.hint = Some(HINT.into());
    Session::new(config)
}

fn push(s: &mut Session, samples: &[f32]) -> Vec<Effect> {
    samples.chunks(128).flat_map(|c| s.push_audio(c)).collect()
}

/// Press and grant the mic.
fn begin(s: &mut Session) -> Vec<Effect> {
    let mut fx = s.handle(Event::Press);
    fx.extend(s.handle(Event::MicGranted {
        sample_rate: MIC_RATE,
    }));
    fx
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn fixture_lines(name: &str) -> Vec<String> {
    fixture(name).lines().map(str::to_owned).collect()
}

fn ws_open(fx: &[Effect]) -> (u32, String, Vec<String>) {
    fx.iter()
        .find_map(|e| match e {
            Effect::WsOpen { id, url, protocols } => Some((*id, url.clone(), protocols.clone())),
            _ => None,
        })
        .expect("WsOpen effect")
}

fn http_request(fx: &[Effect]) -> HttpRequest {
    fx.iter()
        .find_map(|e| match e {
            Effect::HttpRequest(r) => Some(r.clone()),
            _ => None,
        })
        .expect("HttpRequest effect")
}

fn sent_texts(fx: &[Effect], socket: u32) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::WsSend {
                id,
                data: WsData::Text(t),
            } if *id == socket => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn sent_binaries(fx: &[Effect], socket: u32) -> Vec<Vec<u8>> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::WsSend {
                id,
                data: WsData::Binary(b),
            } if *id == socket => Some(b.clone()),
            _ => None,
        })
        .collect()
}

fn interims(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Interim { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn finals(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Final { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn feed_ws(s: &mut Session, socket: u32, lines: &[String]) -> Vec<Effect> {
    lines
        .iter()
        .flat_map(|l| {
            s.handle(Event::WsMessage {
                id: socket,
                data: WsData::Text(l.clone()),
            })
        })
        .collect()
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid JSON {text}: {e}"))
}

fn b64decode(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=') {
        acc = (acc << 6) | A.iter().position(|&a| a == c).expect("base64 char") as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn text_part<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
    let HttpBody::Multipart(parts) = &req.body else {
        panic!("expected multipart body")
    };
    parts
        .iter()
        .find(|p| p.name == name)
        .map(|p| match &p.value {
            PartValue::Text(t) => t.as_str(),
            other => panic!("part {name} is not text: {other:?}"),
        })
}

fn groq() -> Provider {
    Provider::Groq(Groq::relay("https://app.example/api/wordink/"))
}
fn openai() -> Provider {
    Provider::OpenAi(OpenAi::new("ek_test_secret"))
}
fn deepgram() -> Provider {
    Provider::Deepgram(Deepgram::new("dg.jwt.token"))
}
fn host(streaming: bool, sample_rate: u32) -> Provider {
    Provider::Host(HostProvider::new(
        "moonshine",
        Capabilities {
            streaming,
            sample_rate,
        },
    ))
}

/// Runs press, grant, 500 ms of speech and release; returns all effects.
fn record(s: &mut Session) -> Vec<Effect> {
    let mut fx = begin(s);
    fx.extend(push(s, &tone(500, 0.5)));
    fx.extend(s.handle(Event::Release));
    fx
}

// ------------------------------------------------------------- capability

#[test]
fn capabilities_match_ktd4_and_ktd10() {
    let c = groq().capabilities();
    assert!(!c.streaming);
    assert_eq!(c.sample_rate, 16_000);
    let c = openai().capabilities();
    assert!(c.streaming);
    assert_eq!(c.sample_rate, 24_000);
    let c = deepgram().capabilities();
    assert!(c.streaming);
    assert_eq!(c.sample_rate, 16_000);
    let c = host(true, 22_050).capabilities();
    assert!(c.streaming);
    assert_eq!(c.sample_rate, 22_050);
}

// ------------------------------------------------------------------- Groq

#[test]
fn groq_request_carries_model_hint_and_wav_and_text_body_is_final() {
    let mut s = session(groq());
    let fx = record(&mut s);
    assert!(interims(&fx).is_empty(), "Groq is batch-only (KTD10)");
    assert_eq!(s.state(), State::Transcribing);

    let req = http_request(&fx);
    assert_eq!(req.method, "POST");
    assert_eq!(
        req.url,
        "https://app.example/api/wordink/groq/transcriptions"
    );
    assert!(
        req.headers.iter().all(|(k, _)| k != "Authorization"),
        "relay mode must not carry a provider key"
    );
    assert_eq!(text_part(&req, "model"), Some("whisper-large-v3-turbo"));
    assert_eq!(text_part(&req, "response_format"), Some("text"));
    assert_eq!(text_part(&req, "prompt"), Some(HINT));
    assert_eq!(text_part(&req, "language"), None);

    let HttpBody::Multipart(parts) = &req.body else {
        unreachable!()
    };
    let file = parts.iter().find(|p| p.name == "file").expect("file part");
    let PartValue::File {
        filename,
        content_type,
        data,
    } = &file.value
    else {
        panic!("file part is not a file")
    };
    assert_eq!(filename, "audio.wav");
    assert_eq!(content_type, "audio/wav");
    assert_eq!(&data[0..4], b"RIFF");
    assert_eq!(&data[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(data[24..28].try_into().unwrap()), 16_000);
    // 500 ms of 16 kHz mono PCM16 after the 44-byte header.
    let samples = (data.len() - 44) / 2;
    assert!((samples as i64 - 8_000).abs() <= 1, "{samples} samples");

    let done = s.handle(Event::HttpResponse {
        id: req.id,
        status: 200,
        body: fixture("groq_transcription.txt"),
    });
    assert_eq!(
        done,
        vec![
            Effect::Final {
                text: "Hello from WordInk.".into()
            },
            Effect::State(State::Idle)
        ]
    );
}

#[test]
fn groq_dev_key_goes_direct_with_bearer_and_language() {
    let mut g = Groq::dev_key("gsk_dev");
    g.language = Some("en".into());
    g.model = "whisper-large-v3".into();
    let mut s = Session::new(SessionConfig::new(Mode::PushToTalk, Provider::Groq(g)));
    let req = http_request(&record(&mut s));
    assert_eq!(
        req.url,
        "https://api.groq.com/openai/v1/audio/transcriptions"
    );
    assert!(req
        .headers
        .contains(&("Authorization".into(), "Bearer gsk_dev".into())));
    assert_eq!(text_part(&req, "model"), Some("whisper-large-v3"));
    assert_eq!(text_part(&req, "language"), Some("en"));
    // No hint configured, so no prompt part.
    assert_eq!(text_part(&req, "prompt"), None);
}

#[test]
fn groq_stale_response_id_is_ignored() {
    let mut s = session(groq());
    let req = http_request(&record(&mut s));
    let stale = s.handle(Event::HttpResponse {
        id: req.id + 1,
        status: 200,
        body: "nope".into(),
    });
    assert!(stale.is_empty());
    assert_eq!(s.state(), State::Transcribing);
}

#[test]
fn groq_http_errors_map_to_codes() {
    let cases = [
        (401, ErrorCode::AuthFailed),
        (403, ErrorCode::AuthFailed),
        (429, ErrorCode::RateLimited),
        (400, ErrorCode::BadAudio),
        (415, ErrorCode::BadAudio),
        (500, ErrorCode::ProviderDown),
        (503, ErrorCode::ProviderDown),
    ];
    for (status, code) in cases {
        let mut s = session(groq());
        let req = http_request(&record(&mut s));
        let fx = s.handle(Event::HttpResponse {
            id: req.id,
            status,
            body: fixture("groq_error_auth.json"),
        });
        assert_eq!(
            fx,
            vec![Effect::State(State::Error(code))],
            "status {status}"
        );
    }

    let mut s = session(groq());
    let req = http_request(&record(&mut s));
    let fx = s.handle(Event::HttpFailed { id: req.id });
    assert_eq!(
        fx,
        vec![Effect::State(State::Error(ErrorCode::ProviderDown))]
    );
}

#[test]
fn empty_batch_transcript_is_no_speech() {
    let mut s = session(groq());
    let req = http_request(&record(&mut s));
    let fx = s.handle(Event::HttpResponse {
        id: req.id,
        status: 200,
        body: " \n".into(),
    });
    assert_eq!(fx, vec![Effect::State(State::Error(ErrorCode::NoSpeech))]);
}

// ----------------------------------------------------------------- OpenAI

#[test]
fn openai_opens_socket_with_ephemeral_secret_and_configures_session() {
    let mut s = session(openai());
    let fx = begin(&mut s);
    let (id, url, protocols) = ws_open(&fx);
    assert_eq!(url, "wss://api.openai.com/v1/realtime?intent=transcription");
    assert_eq!(
        protocols,
        vec!["realtime", "openai-insecure-api-key.ek_test_secret"]
    );

    // Audio before the socket opens is held, not dropped.
    let early = push(&mut s, &tone(200, 0.5));
    assert!(sent_texts(&early, id).is_empty());

    let opened = s.handle(Event::WsOpened { id });
    let texts = sent_texts(&opened, id);
    let update = json(&texts[0]);
    // The GA transcription session shape (the relay mints GA client secrets).
    assert_eq!(update["type"], "session.update");
    let session = &update["session"];
    assert_eq!(session["type"], "transcription");
    let input = &session["audio"]["input"];
    assert_eq!(
        input["format"],
        json(r#"{"type":"audio/pcm","rate":24000}"#)
    );
    assert_eq!(input["transcription"]["model"], "gpt-live-transcribe");
    assert_eq!(input["transcription"]["prompt"], HINT);
    assert!(input.get("turn_detection").is_some_and(Value::is_null));
    assert!(texts.len() > 1, "held audio is flushed after the update");
    for t in &texts[1..] {
        assert_eq!(json(t)["type"], "input_audio_buffer.append");
    }
}

#[test]
fn openai_streams_24k_pcm16_then_commits_on_release() {
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    let mut fx = s.handle(Event::WsOpened { id });
    fx.extend(push(&mut s, &tone(500, 0.5)));
    let during = sent_texts(&fx, id).len();
    assert!(during > 2, "audio streams while listening (R8)");

    let released = s.handle(Event::Release);
    assert_eq!(released[0], Effect::StopMic);
    assert_eq!(released[1], Effect::State(State::Transcribing));
    fx.extend(released.clone());

    let texts = sent_texts(&fx, id);
    assert_eq!(
        json(texts.last().unwrap()),
        json(r#"{"type":"input_audio_buffer.commit"}"#)
    );
    let mut bytes = 0;
    for t in &texts[1..texts.len() - 1] {
        let v = json(t);
        assert_eq!(v["type"], "input_audio_buffer.append");
        bytes += b64decode(v["audio"].as_str().unwrap()).len();
    }
    // 500 ms of 24 kHz PCM16.
    let samples = bytes / 2;
    assert!((samples as i64 - 12_000).abs() <= 1, "{samples} samples");
}

#[test]
fn openai_delta_then_completed_yields_interims_then_one_final() {
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    s.handle(Event::WsOpened { id });
    push(&mut s, &tone(500, 0.5));
    s.handle(Event::Release);

    let fx = feed_ws(&mut s, id, &fixture_lines("openai_delta_completed.jsonl"));
    assert_eq!(
        interims(&fx),
        vec![
            "Hello",
            "Hello world,",
            "Hello world, how are \"you\"?",
            "Hello world, how are \"you\"? Café.",
        ]
    );
    assert_eq!(finals(&fx), vec!["Hello world, how are \"you\"? Café."]);
    let n = fx.len();
    assert_eq!(fx[n - 3], Effect::WsClose { id });
    assert_eq!(fx[n - 1], Effect::State(State::Idle));
    assert_eq!(s.state(), State::Idle);
}

#[test]
fn openai_error_events_map_to_codes() {
    for (file, code) in [
        ("openai_error_auth.jsonl", ErrorCode::AuthFailed),
        ("openai_error_rate_limit.jsonl", ErrorCode::RateLimited),
    ] {
        let mut s = session(openai());
        let (id, _, _) = ws_open(&begin(&mut s));
        s.handle(Event::WsOpened { id });
        let fx = feed_ws(&mut s, id, &fixture_lines(file));
        assert_eq!(
            fx,
            vec![
                Effect::StopMic,
                Effect::WsClose { id },
                Effect::State(State::Error(code))
            ],
            "{file}"
        );
    }
}

#[test]
fn openai_rejected_handshake_is_auth_failed() {
    // Browsers hide the handshake's HTTP status (a 401 shows up as a close
    // before open), so a socket that never opens maps to AuthFailed.
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    let fx = s.handle(Event::WsClosed { id, code: 1006 });
    assert_eq!(
        fx,
        vec![
            Effect::StopMic,
            Effect::State(State::Error(ErrorCode::AuthFailed))
        ]
    );
}

#[test]
fn openai_stalled_connect_is_provider_down() {
    // The host's private stall code marks a connect timeout (R9): the
    // provider could not be reached, so it did not reject the credential.
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    let fx = s.handle(Event::WsClosed {
        id,
        code: WS_CLOSE_STALL,
    });
    assert_eq!(
        fx,
        vec![
            Effect::StopMic,
            Effect::State(State::Error(ErrorCode::ProviderDown))
        ]
    );
}

#[test]
fn openai_without_hint_omits_prompt() {
    let mut s = Session::new(SessionConfig::new(Mode::PushToTalk, openai()));
    let (id, _, _) = ws_open(&begin(&mut s));
    let update = json(&sent_texts(&s.handle(Event::WsOpened { id }), id)[0]);
    let transcription = &update["session"]["audio"]["input"]["transcription"];
    assert_eq!(transcription["model"], "gpt-live-transcribe");
    assert!(transcription.get("prompt").is_none());
}

/// Delta and completed lines from the fixture: one finished item.
fn openai_item() -> Vec<String> {
    fixture_lines("openai_delta_completed.jsonl")
        .into_iter()
        .filter(|l| l.contains("input_audio_transcription"))
        .collect()
}

#[test]
fn openai_completed_before_release_still_finalizes_on_release() {
    // A completed item while the button is still held keeps the socket open
    // and shows as interim text; the commit on release produces the final.
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    s.handle(Event::WsOpened { id });
    push(&mut s, &tone(500, 0.5));
    let early = feed_ws(&mut s, id, &openai_item());
    assert!(
        !early.contains(&Effect::WsClose { id }),
        "socket stays open"
    );
    assert!(finals(&early).is_empty());
    assert_eq!(
        interims(&early).last().unwrap(),
        "Hello world, how are \"you\"? Café."
    );
    assert_eq!(s.state(), State::Listening);

    push(&mut s, &tone(500, 0.5));
    let released = s.handle(Event::Release);
    assert_eq!(
        json(sent_texts(&released, id).last().unwrap()),
        json(r#"{"type":"input_audio_buffer.commit"}"#)
    );
    assert_eq!(s.state(), State::Transcribing);

    let delta = r#"{"type":"conversation.item.input_audio_transcription.delta","item_id":"item_002","content_index":0,"delta":"More."}"#;
    let completed = r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"item_002","content_index":0,"transcript":"More."}"#;
    let fx = feed_ws(&mut s, id, &[delta.into(), completed.into()]);
    assert_eq!(
        interims(&fx),
        vec!["Hello world, how are \"you\"? Café. More."]
    );
    assert_eq!(
        finals(&fx),
        vec!["Hello world, how are \"you\"? Café. More."]
    );
    assert!(fx.contains(&Effect::WsClose { id }));
    assert_eq!(s.state(), State::Idle);
}

#[test]
fn openai_completed_before_release_then_empty_commit_is_final() {
    // If the provider already transcribed everything, the commit on release
    // finds an empty buffer; the text so far is the final.
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    s.handle(Event::WsOpened { id });
    push(&mut s, &tone(500, 0.5));
    feed_ws(&mut s, id, &openai_item());
    s.handle(Event::Release);
    let empty = r#"{"type":"error","error":{"type":"invalid_request_error","code":"input_audio_buffer_commit_empty","message":"buffer too small"}}"#;
    let fx = feed_ws(&mut s, id, &[empty.into()]);
    assert_eq!(finals(&fx), vec!["Hello world, how are \"you\"? Café."]);
    assert_eq!(fx.last(), Some(&Effect::State(State::Idle)));
}

// --------------------------------------------------------------- Deepgram

#[test]
fn deepgram_url_carries_linear16_params_and_keyterms() {
    let mut s = session(deepgram());
    let (_, url, protocols) = ws_open(&begin(&mut s));
    assert_eq!(
        url,
        "wss://api.deepgram.com/v1/listen?encoding=linear16&sample_rate=16000&channels=1\
         &interim_results=true&model=nova-3&keyterm=WordInk&keyterm=Omarchy"
    );
    assert_eq!(protocols, vec!["token", "dg.jwt.token"]);
}

#[test]
fn deepgram_keyterms_are_percent_encoded() {
    let mut config = SessionConfig::new(Mode::PushToTalk, deepgram());
    config.hint = Some("Kubernetes pod\nnaïve & co".into());
    let mut s = Session::new(config);
    let (_, url, _) = ws_open(&begin(&mut s));
    assert!(
        url.ends_with("&keyterm=Kubernetes%20pod&keyterm=na%C3%AFve%20%26%20co"),
        "{url}"
    );
}

#[test]
fn deepgram_falls_back_to_bearer_subprotocol() {
    let mut s = session(deepgram());
    let (first, _, _) = ws_open(&begin(&mut s));
    let retry = s.handle(Event::WsClosed {
        id: first,
        code: 1006,
    });
    let (second, _, protocols) = ws_open(&retry);
    assert_ne!(first, second);
    assert_eq!(protocols, vec!["bearer", "dg.jwt.token"]);
    assert_eq!(s.state(), State::Listening);

    // Messages for the abandoned socket are ignored.
    assert!(feed_ws(
        &mut s,
        first,
        &fixture_lines("deepgram_interim_final.jsonl")
    )
    .is_empty());

    let fx = s.handle(Event::WsClosed {
        id: second,
        code: 1006,
    });
    assert_eq!(
        fx,
        vec![
            Effect::StopMic,
            Effect::State(State::Error(ErrorCode::AuthFailed))
        ]
    );
}

#[test]
fn deepgram_stalled_candidate_retries_then_is_provider_down() {
    // A mid-chain stall is like any close before open: the next subprotocol
    // candidate gets its own attempt.
    let mut s = session(deepgram());
    let (first, _, _) = ws_open(&begin(&mut s));
    let retry = s.handle(Event::WsClosed {
        id: first,
        code: WS_CLOSE_STALL,
    });
    let (second, _, protocols) = ws_open(&retry);
    assert_ne!(first, second);
    assert_eq!(protocols, vec!["bearer", "dg.jwt.token"]);
    assert_eq!(s.state(), State::Listening);

    // The last candidate stalling out ends in ProviderDown, not AuthFailed.
    let fx = s.handle(Event::WsClosed {
        id: second,
        code: WS_CLOSE_STALL,
    });
    assert_eq!(
        fx,
        vec![
            Effect::StopMic,
            Effect::State(State::Error(ErrorCode::ProviderDown))
        ]
    );
}

#[test]
fn deepgram_interim_then_final_matches_fixture() {
    let lines = fixture_lines("deepgram_interim_final.jsonl");
    let mut s = session(deepgram());
    let (id, _, _) = ws_open(&begin(&mut s));
    let mut fx = s.handle(Event::WsOpened { id });
    fx.extend(push(&mut s, &tone(500, 0.5)));

    // Interim text arrives while still listening (R8).
    let first = feed_ws(&mut s, id, &lines[..1]);
    assert_eq!(interims(&first), vec!["hello"]);
    assert_eq!(s.state(), State::Listening);

    let released = s.handle(Event::Release);
    fx.extend(released.clone());
    assert_eq!(
        sent_texts(&released, id),
        vec![r#"{"type":"CloseStream"}"#.to_string()]
    );
    let bytes: usize = sent_binaries(&fx, id).iter().map(Vec::len).sum();
    let samples = bytes / 2;
    assert!((samples as i64 - 8_000).abs() <= 1, "{samples} samples");

    let rest = feed_ws(&mut s, id, &lines[1..]);
    assert_eq!(
        interims(&rest),
        vec![
            "hello world",
            "hello world how are",
            "hello world how are you"
        ]
    );
    assert!(
        finals(&rest).is_empty(),
        "final waits for the stream to close"
    );

    let done = s.handle(Event::WsClosed { id, code: 1000 });
    assert_eq!(
        done,
        vec![
            Effect::Final {
                text: "hello world how are you".into()
            },
            Effect::State(State::Idle)
        ]
    );
}

#[test]
fn deepgram_empty_final_is_no_speech() {
    let mut s = session(deepgram());
    let (id, _, _) = ws_open(&begin(&mut s));
    s.handle(Event::WsOpened { id });
    push(&mut s, &tone(500, 0.5));
    s.handle(Event::Release);
    let fx = feed_ws(&mut s, id, &fixture_lines("deepgram_empty_final.jsonl"));
    assert!(interims(&fx).is_empty());
    let done = s.handle(Event::WsClosed { id, code: 1000 });
    assert_eq!(done, vec![Effect::State(State::Error(ErrorCode::NoSpeech))]);
}

#[test]
fn deepgram_close_codes_map_to_codes() {
    for (code, err) in [
        (1008, ErrorCode::BadAudio),
        (1011, ErrorCode::ProviderDown),
        (3000, ErrorCode::AuthFailed),
        (1013, ErrorCode::RateLimited),
    ] {
        let mut s = session(deepgram());
        let (id, _, _) = ws_open(&begin(&mut s));
        s.handle(Event::WsOpened { id });
        push(&mut s, &tone(500, 0.5));
        let fx = s.handle(Event::WsClosed { id, code });
        assert_eq!(
            fx,
            vec![Effect::StopMic, Effect::State(State::Error(err))],
            "close {code}"
        );
    }
}

#[test]
fn streaming_no_speech_closes_the_socket() {
    let mut s = session(deepgram());
    let (id, _, _) = ws_open(&begin(&mut s));
    s.handle(Event::WsOpened { id });
    push(&mut s, &tone(200, 0.5));
    assert_eq!(
        s.handle(Event::Release),
        vec![
            Effect::StopMic,
            Effect::WsClose { id },
            Effect::State(State::Error(ErrorCode::NoSpeech))
        ]
    );
}

// ------------------------------------------------------- provider switch

fn is_provider_effect(e: &Effect) -> bool {
    !matches!(
        e,
        Effect::State(_) | Effect::RequestMic | Effect::StopMic | Effect::Level { .. }
    )
}

#[test]
fn switching_provider_changes_only_provider_effects() {
    let runs: Vec<(Provider, Vec<Effect>)> = [groq(), openai(), deepgram(), host(false, 16_000)]
        .into_iter()
        .map(|p| {
            let mut s = session(p.clone());
            (p, record(&mut s))
        })
        .collect();

    let session_only = |fx: &[Effect]| -> Vec<Effect> {
        fx.iter()
            .filter(|e| !is_provider_effect(e))
            .cloned()
            .collect()
    };
    let baseline = session_only(&runs[0].1);
    assert!(baseline.contains(&Effect::State(State::Transcribing)));
    for (p, fx) in &runs {
        assert_eq!(session_only(fx), baseline, "{p:?}");
        let provider_fx: Vec<&Effect> = fx.iter().filter(|e| is_provider_effect(e)).collect();
        let expected = match p {
            Provider::Groq(_) => matches!(provider_fx[..], [Effect::HttpRequest(_)]),
            Provider::OpenAi(_) | Provider::Deepgram(_) => {
                matches!(provider_fx[..], [Effect::WsOpen { .. }])
            }
            Provider::Host(_) => matches!(
                provider_fx[..],
                [
                    Effect::HostProviderStart { .. },
                    Effect::HostProviderAudio { .. },
                    Effect::HostProviderFinish { .. }
                ]
            ),
            _ => unreachable!(),
        };
        assert!(expected, "{p:?}: {provider_fx:?}");
    }
}

// ------------------------------------------------------- host-delegated

fn host_audio(fx: &[Effect]) -> usize {
    fx.iter()
        .map(|e| match e {
            Effect::HostProviderAudio { id, samples } => {
                assert_eq!(id, "moonshine");
                samples.len()
            }
            _ => 0,
        })
        .sum()
}

#[test]
fn streaming_host_provider_gets_audio_at_declared_rate() {
    for (rate, expected) in [(16_000, 8_000i64), (24_000, 12_000)] {
        let mut s = session(host(true, rate));
        let started = begin(&mut s);
        assert_eq!(
            started.last(),
            Some(&Effect::HostProviderStart {
                id: "moonshine".into(),
                sample_rate: rate,
                hint: Some(HINT.into()),
            })
        );
        let listening = push(&mut s, &tone(500, 0.5));
        assert!(
            host_audio(&listening) > 0,
            "audio is delivered while listening"
        );

        let interim = s.handle(Event::HostProviderResult(HostResult::Interim {
            text: "hello".into(),
        }));
        assert_eq!(
            interim,
            vec![Effect::Interim {
                text: "hello".into()
            }]
        );

        let released = s.handle(Event::Release);
        assert_eq!(
            released.last(),
            Some(&Effect::HostProviderFinish {
                id: "moonshine".into()
            })
        );
        let total = host_audio(&listening) + host_audio(&released);
        assert!((total as i64 - expected).abs() <= 1, "{rate}: {total}");

        let done = s.handle(Event::HostProviderResult(HostResult::Final {
            text: "hello world".into(),
        }));
        assert_eq!(
            done,
            vec![
                Effect::Final {
                    text: "hello world".into()
                },
                Effect::State(State::Idle)
            ]
        );
    }
}

#[test]
fn batch_host_provider_gets_the_whole_utterance_on_release() {
    let mut s = session(host(false, 16_000));
    let mut fx = begin(&mut s);
    fx.extend(push(&mut s, &tone(500, 0.5)));
    assert!(!fx.iter().any(is_provider_effect));
    let released = s.handle(Event::Release);
    assert!((host_audio(&released) as i64 - 8_000).abs() <= 1);
}

#[test]
fn host_provider_error_and_cancel() {
    let mut s = session(host(true, 16_000));
    begin(&mut s);
    push(&mut s, &tone(500, 0.5));
    let fx = s.handle(Event::HostProviderResult(HostResult::Error {
        code: ErrorCode::ProviderDown,
    }));
    assert_eq!(
        fx,
        vec![
            Effect::StopMic,
            Effect::State(State::Error(ErrorCode::ProviderDown))
        ]
    );

    // No speech: the host provider is told to drop what it has.
    let mut s = session(host(true, 16_000));
    begin(&mut s);
    push(&mut s, &tone(100, 0.5));
    assert_eq!(
        s.handle(Event::Release),
        vec![
            Effect::StopMic,
            Effect::HostProviderCancel {
                id: "moonshine".into()
            },
            Effect::State(State::Error(ErrorCode::NoSpeech))
        ]
    );
}

#[test]
fn streaming_host_final_before_release_completes_on_release() {
    // A streaming host provider that reports its final while the button is
    // held has finished: release completes with that text, not a hang.
    let mut s = session(host(true, 16_000));
    begin(&mut s);
    push(&mut s, &tone(500, 0.5));
    let early = s.handle(Event::HostProviderResult(HostResult::Final {
        text: "hello world".into(),
    }));
    assert_eq!(
        early,
        vec![Effect::Interim {
            text: "hello world".into()
        }]
    );
    assert_eq!(s.state(), State::Listening);

    let released = s.handle(Event::Release);
    assert_eq!(finals(&released), vec!["hello world"]);
    assert_eq!(released.last(), Some(&Effect::State(State::Idle)));
    assert_eq!(s.state(), State::Idle);
}

// ------------------------------------------------------ utterance limit

#[test]
fn utterance_auto_stops_at_the_limit_and_groq_gets_at_most_60_s() {
    assert_eq!(wordink_core::session::MAX_UTTERANCE_MS, 60_000);
    let config = SessionConfig::new(Mode::PushToTalk, groq());
    assert_eq!(config.max_utterance_ms, 60_000);

    let mut s = session(groq());
    begin(&mut s);
    let audio = tone(61_000, 0.5);
    let mut fx = Vec::new();
    let mut stopped_at = None;
    for (i, chunk) in audio.chunks(128).enumerate() {
        let out = s.push_audio(chunk);
        if stopped_at.is_none() && out.contains(&Effect::State(State::Transcribing)) {
            stopped_at = Some((i + 1) * 128);
        }
        fx.extend(out);
    }
    // Stops on its own, in the chunk that reaches 60 s, without a release.
    let stopped_at = stopped_at.expect("auto-stop while holding");
    assert!(stopped_at >= 60 * MIC_RATE as usize && stopped_at < 60 * MIC_RATE as usize + 128);
    assert_eq!(s.state(), State::Transcribing);
    assert_eq!(fx.iter().filter(|e| **e == Effect::StopMic).count(), 1);

    let req = http_request(&fx);
    let HttpBody::Multipart(parts) = &req.body else {
        unreachable!()
    };
    let PartValue::File { data, .. } = &parts[0].value else {
        panic!("file part")
    };
    let samples = (data.len() - 44) / 2;
    assert!(samples <= 60 * 16_000, "{samples} samples");
    assert!(samples >= 60 * 16_000 - 16, "{samples} samples");

    // Release after the auto-stop does nothing.
    assert!(s.handle(Event::Release).is_empty());
}

#[test]
fn utterance_limit_is_configurable() {
    let mut config = SessionConfig::new(Mode::Toggle, groq());
    config.max_utterance_ms = 1_000;
    let mut s = Session::new(config);
    begin(&mut s);
    let fx = push(&mut s, &tone(1_500, 0.5));
    assert_eq!(s.state(), State::Transcribing);
    let req = http_request(&fx);
    let HttpBody::Multipart(parts) = &req.body else {
        unreachable!()
    };
    let PartValue::File { data, .. } = &parts[0].value else {
        panic!("file part")
    };
    assert!((data.len() - 44) / 2 <= 16_000);
}

#[test]
fn openai_streaming_auto_stops_at_the_limit_and_sends_at_most_60_s() {
    // The cap ends a streaming utterance the same way it ends a batch one: capture stops and the
    // provider finalizes with what it has (R11). The wire carries only the capped audio + commit.
    let mut s = session(openai());
    let (id, _, _) = ws_open(&begin(&mut s));
    s.handle(Event::WsOpened { id });
    let audio = tone(61_000, 0.5);
    let mut fx = Vec::new();
    let mut stopped_at = None;
    for (i, chunk) in audio.chunks(128).enumerate() {
        let out = s.push_audio(chunk);
        if stopped_at.is_none() && out.contains(&Effect::State(State::Transcribing)) {
            stopped_at = Some((i + 1) * 128);
        }
        fx.extend(out);
    }
    // Stops on its own, in the chunk that reaches 60 s, without a release.
    let stopped_at = stopped_at.expect("auto-stop while holding");
    assert!(stopped_at >= 60 * MIC_RATE as usize && stopped_at < 60 * MIC_RATE as usize + 128);
    assert_eq!(s.state(), State::Transcribing);
    assert_eq!(fx.iter().filter(|e| **e == Effect::StopMic).count(), 1);

    let texts = sent_texts(&fx, id);
    assert_eq!(
        json(texts.last().unwrap()),
        json(r#"{"type":"input_audio_buffer.commit"}"#)
    );
    let mut bytes = 0;
    for t in &texts[1..texts.len() - 1] {
        let v = json(t);
        assert_eq!(v["type"], "input_audio_buffer.append");
        bytes += b64decode(v["audio"].as_str().unwrap()).len();
    }
    // At most 60 s of 24 kHz PCM16 reached the provider.
    assert!(bytes / 2 <= 24_000 * 60, "{} samples", bytes / 2);

    // Release after the auto-stop does nothing.
    assert!(s.handle(Event::Release).is_empty());
}
