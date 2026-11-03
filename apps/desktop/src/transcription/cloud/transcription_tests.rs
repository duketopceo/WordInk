//! OpenRouter's transcription endpoint: the request, the reply, the errors,
//! the retries, the price, the request log, and the catalogue that lists the
//! models. Every request goes to a local mock server; nothing here is a live
//! call.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{Value, json};

use super::request_log::{self, Origin};
use super::*;

const MAI: &str = "microsoft/mai-transcribe-2";

/// What the mock saw of one request.
struct Seen {
    head: String,
    body: Vec<u8>,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

/// A local server that answers one request per entry of `replies` (status,
/// extra headers, body), then sends what it saw.
type Reply = (u16, Vec<(&'static str, &'static str)>, String);

fn mock_sequence(
    replies: Vec<Reply>,
) -> (String, mpsc::Receiver<Vec<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/api/v1/audio/transcriptions",
        listener.local_addr().unwrap()
    );
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut seen = Vec::new();
        for (status, headers, body) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                head.push_str(&line);
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            let extra: String = headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect();
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
            seen.push(Seen { head, body: request });
        }
        let _ = sender.send(seen);
    });
    (url, receiver)
}

fn one(
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
    body: &str,
) -> (String, mpsc::Receiver<Vec<Seen>>) {
    mock_sequence(vec![(status, headers, body.to_owned())])
}

fn speech() -> Vec<f32> {
    (0..16_000 * 2)
        .map(|i| ((i as f32) * 0.05).sin() * 0.3)
        .collect()
}

fn temp_log(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wordink-stt-{name}-{}-{}",
        std::process::id(),
        crate::history::now_ms()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(request_log::FILE)
}

fn stt_call<'a>(url: &'a str, model: &'a str) -> Call<'a> {
    let mut call = Call::new(Provider::OpenRouter, url, model);
    call.key = Some("or-secret-key-123");
    call.kind = openrouter::Kind::Transcription;
    call
}

const PLAIN: &str = r#"{"text":" She taught her daughter. ","usage":{"seconds":2,"cost":0.0000555}}"#;

#[test]
fn the_request_is_json_with_the_model_and_the_clip_as_base64_wav() {
    let wav = wav_bytes(&speech());
    let body = openrouter::transcription_body(MAI, &wav, None, openrouter::Detail::Plain);
    assert_eq!(body["model"], MAI);
    assert_eq!(body["input_audio"]["format"], "wav");
    assert_eq!(body["input_audio"]["data"], openrouter::base64(&wav));
    assert!(
        body["input_audio"]["data"].as_str().unwrap().starts_with("UklGR"),
        "RIFF"
    );
    assert!(body.get("language").is_none() && body.get("response_format").is_none());
    let body = openrouter::transcription_body(MAI, &wav, Some("en"), openrouter::Detail::Verbose);
    assert_eq!(body["language"], "en");
    assert_eq!(body["response_format"], "verbose_json");
}

#[test]
fn a_transcription_goes_to_the_endpoint_with_a_bearer_key_and_returns_text_and_cost() {
    let (url, seen) = one(200, vec![], PLAIN);
    let log = temp_log("wire");
    let mut call = stt_call(&url, MAI);
    call.language = Some("en");
    call.request_log = Some(&log);
    let outcome = run_outcome_at(&call, &speech(), None).unwrap();
    assert_eq!(outcome.text, "She taught her daughter.");
    assert_eq!(outcome.usage.provider, "openrouter");
    assert_eq!(outcome.usage.model, MAI);
    assert!((outcome.usage.audio_seconds - 2.0).abs() < 1e-9);
    assert!(
        (outcome.usage.cost_usd - 0.0000555).abs() < 1e-12,
        "the cost the provider returned"
    );
    let seen = seen.recv().unwrap();
    let request = &seen[0];
    assert!(request.head.starts_with("POST /api/v1/audio/transcriptions"));
    assert!(request.head.to_ascii_lowercase().contains("content-type: application/json"));
    assert!(request.head.contains("Bearer or-secret-key-123"));
    assert_eq!(request.json()["model"], MAI);
    assert_eq!(request.json()["language"], "en");
}

#[test]
fn a_reply_without_a_cost_is_priced_from_the_catalogue_then_the_bundled_table() {
    let no_cost = r#"{"text":"hello","usage":{"seconds":3600}}"#;
    // The catalogue's hourly price, for a model the table does not list.
    let catalogue = openrouter::Model {
        id: "acme/stt".into(),
        name: "Acme STT".into(),
        prompt_usd: 0.0,
        completion_usd: 0.0,
        audio_usd: None,
        kind: openrouter::Kind::Transcription,
        usd_per_hour: Some(0.36),
    };
    let (url, _seen) = one(200, vec![], no_cost);
    let mut call = stt_call(&url, "acme/stt");
    call.catalogue = Some(&catalogue);
    let outcome = run_outcome_at(&call, &speech(), None).unwrap();
    assert!((outcome.usage.cost_usd - 0.36).abs() < 1e-9, "an hour at $0.36/h");
    // The bundled table has MAI at $0.10/hour with no catalogue at all.
    let (url, _seen) = one(200, vec![], no_cost);
    let call = stt_call(&url, MAI);
    let outcome = run_outcome_at(&call, &speech(), None).unwrap();
    assert!((outcome.usage.cost_usd - 0.10).abs() < 1e-9, "an hour at $0.10/h");
    // A model neither lists costs nothing known.
    let (url, _seen) = one(200, vec![], no_cost);
    let call = stt_call(&url, "acme/unpriced");
    assert_eq!(run_outcome_at(&call, &speech(), None).unwrap().usage.cost_usd, 0.0);
}

#[test]
fn verbose_json_gives_segments_and_words_and_leaves_out_sound_tags() {
    let reply = json!({
        "text": "Hello there. Hi.",
        "language": "english",
        "duration": 2.0,
        "segments": [
            { "id": 0, "start": 0.0, "end": 1.2, "text": " Hello there. " },
            { "id": 1, "start": 1.3, "end": 2.0, "text": "Hi." },
            { "id": 2, "start": 2.0, "end": 2.0, "text": "  " },
        ],
        "words": [
            { "word": "Hello", "start": 0.0, "end": 0.4, "speaker": 0 },
            { "word": "(laughter)", "start": 0.5, "end": 0.9, "type": "audio_event" },
            { "word": "there.", "start": 0.4, "end": 1.2, "type": "word" },
        ],
        "usage": { "seconds": 2.0, "cost": 0.00005, "total_tokens": 7 },
    });
    let parsed = openrouter::parse_transcription(&reply).unwrap();
    assert_eq!(parsed.text, "Hello there. Hi.");
    let segments: Vec<_> = parsed
        .segments
        .iter()
        .map(|s| (s.start_ms, s.end_ms, s.text.as_str()))
        .collect();
    assert_eq!(segments, [(0, 1200, "Hello there."), (1300, 2000, "Hi.")]);
    let words: Vec<_> = parsed.words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(words, ["Hello", "there."]);
    assert_eq!(parsed.billed_seconds, Some(2.0));
    assert_eq!(parsed.cost_usd, Some(0.00005));
    assert_eq!(parsed.total_tokens, Some(7));
    // Plain JSON has neither timing.
    let plain = openrouter::parse_transcription(&json!({"text":"x"})).unwrap();
    assert!(plain.segments.is_empty() && plain.words.is_empty() && plain.cost_usd.is_none());
    // No text is no transcript; a bad cost is not a cost.
    assert!(openrouter::parse_transcription(&json!({"usage":{}})).is_none());
    assert!(openrouter::parse_transcription(&json!({"text":"x","usage":{"cost":-1}})).unwrap().cost_usd.is_none());
}

#[test]
fn a_meeting_asks_for_verbose_json_and_a_dictation_does_not() {
    let (url, _) = one(200, vec![], "{}");
    let mut dictation = stt_call(&url, MAI);
    tune(&mut dictation, openrouter::Kind::Transcription, Origin::Dictation);
    assert_eq!(dictation.detail, openrouter::Detail::Plain);
    assert_eq!((dictation.max_retries, dictation.origin), (1, Origin::Dictation));
    let mut meeting = stt_call(&url, MAI);
    tune(&mut meeting, openrouter::Kind::Transcription, Origin::Meeting);
    assert_eq!(meeting.detail, openrouter::Detail::Verbose);
    assert_eq!(meeting.origin, Origin::Meeting);
    // A chat model keeps the chat request: no retries, no verbose JSON.
    let mut chat = Call::new(Provider::OpenRouter, &url, "google/gemini-2.5-flash");
    tune(&mut chat, openrouter::Kind::Chat, Origin::Meeting);
    assert_eq!((chat.detail, chat.max_retries), (openrouter::Detail::Plain, 0));
}

#[test]
fn http_failures_read_as_clear_messages_for_the_transcription_endpoint() {
    for (status, expected) in [
        (401, "OpenRouter key rejected"),
        (403, "OpenRouter key rejected"),
        (402, "OpenRouter account is out of credit"),
        (404, "OpenRouter has no provider for this model right now"),
        (429, "OpenRouter rate limit \u{2014} try again shortly"),
        (500, "OpenRouter is unavailable"),
        (503, "OpenRouter is unavailable"),
        (400, "OpenRouter could not transcribe this clip (HTTP 400)"),
    ] {
        let (url, _seen) = one(
            status,
            vec![],
            r#"{"error":{"code":0,"message":"upstream said no"}}"#,
        );
        let mut call = stt_call(&url, MAI);
        call.max_retries = 0;
        let error = run_at(&call, &speech(), None).unwrap_err();
        assert_eq!(error, expected, "HTTP {status}");
        assert!(!error.contains("or-secret-key-123") && !error.contains("upstream said no"));
    }
}

#[test]
fn an_error_body_on_a_200_is_the_error_it_names() {
    for (code, expected) in [
        (402, "OpenRouter account is out of credit"),
        (429, "OpenRouter rate limit \u{2014} try again shortly"),
        (503, "OpenRouter is unavailable"),
    ] {
        let body = format!(r#"{{"error":{{"code":{code},"message":"x"}}}}"#);
        let (url, _seen) = one(200, vec![], &body);
        let call = stt_call(&url, MAI);
        assert_eq!(run_at(&call, &speech(), None).unwrap_err(), expected);
    }
    // A reply with neither text nor error is unreadable.
    let (url, _seen) = one(200, vec![], r#"{"unexpected":true}"#);
    assert_eq!(
        run_at(&stt_call(&url, MAI), &speech(), None).unwrap_err(),
        "OpenRouter sent an unreadable reply"
    );
    let parsed = openrouter::parse_error(r#"{"error":{"code":402,"message":"Insufficient credits"}}"#);
    assert_eq!(parsed.map(|e| e.code), Some(Some(402)));
    assert!(openrouter::parse_error(r#"{"text":"x"}"#).is_none());
    assert!(openrouter::parse_error("not json").is_none());
}

#[test]
fn a_rate_limit_is_retried_once_and_the_retry_is_counted() {
    let log = temp_log("retry");
    let (url, seen) = mock_sequence(vec![
        (429, vec![], "{}".into()),
        (200, vec![], PLAIN.into()),
    ]);
    let mut call = stt_call(&url, MAI);
    call.max_retries = 1;
    call.request_log = Some(&log);
    assert!(run_at(&call, &speech(), None).is_ok());
    assert_eq!(seen.recv().unwrap().len(), 2);
    let records = request_log::read(&log);
    assert_eq!(records.len(), 1, "one record for the whole request");
    assert_eq!((records[0].retries, records[0].status), (1, Some(200)));
    assert!(records[0].ok());

    // Two failures: the second is the answer, and it is logged as a failure.
    let (url, _seen) = mock_sequence(vec![
        (503, vec![], "{}".into()),
        (503, vec![], "{}".into()),
    ]);
    let mut call = stt_call(&url, MAI);
    call.max_retries = 1;
    call.request_log = Some(&log);
    assert_eq!(
        run_at(&call, &speech(), None).unwrap_err(),
        "OpenRouter is unavailable"
    );
    let records = request_log::read(&log);
    assert_eq!(records[1].retries, 1);
    assert_eq!(records[1].error_class.as_deref(), Some("unavailable"));
    assert_eq!(records[1].status, Some(503));

    // A key the provider rejects is not retried.
    let (url, seen) = mock_sequence(vec![(401, vec![], "{}".into())]);
    let mut call = stt_call(&url, MAI);
    call.max_retries = 3;
    assert!(run_at(&call, &speech(), None).is_err());
    assert_eq!(seen.recv().unwrap().len(), 1);
}

#[test]
fn a_refused_verbose_request_is_sent_again_as_plain_json() {
    let (url, seen) = mock_sequence(vec![
        (400, vec![], r#"{"error":{"code":400,"message":"verbose_json unsupported"}}"#.into()),
        (200, vec![], PLAIN.into()),
    ]);
    let mut call = stt_call(&url, MAI);
    call.detail = openrouter::Detail::Verbose;
    let outcome = run_outcome_at(&call, &speech(), None).unwrap();
    assert_eq!(outcome.text, "She taught her daughter.");
    let seen = seen.recv().unwrap();
    assert_eq!(seen[0].json()["response_format"], "verbose_json");
    assert!(seen[1].json().get("response_format").is_none());
}

#[test]
fn every_request_is_logged_with_its_ids_size_time_and_cost_but_no_secret_or_text() {
    let log = temp_log("fields");
    let (url, _seen) = one(
        200,
        vec![
            ("X-Generation-Id", "gen-stt-123-abc"),
            ("X-Provider-Name", "Azure"),
            ("request-id", "req-9"),
        ],
        PLAIN,
    );
    let mut call = stt_call(&url, MAI);
    call.request_log = Some(&log);
    call.origin = Origin::Meeting;
    run_at(&call, &speech(), None).unwrap();
    let records = request_log::read(&log);
    let record = &records[0];
    assert_eq!(record.provider, "openrouter");
    assert_eq!(record.endpoint_kind, "transcription");
    assert_eq!(record.model, MAI);
    assert_eq!(record.origin, Origin::Meeting);
    assert_eq!(record.audio_ms, 2_000);
    assert!(record.request_bytes > 64_000, "the base64 WAV body");
    assert_eq!(record.status, Some(200));
    assert_eq!(record.request_id.as_deref(), Some("req-9"));
    assert_eq!(record.generation_id.as_deref(), Some("gen-stt-123-abc"));
    assert_eq!(record.routed_provider.as_deref(), Some("Azure"));
    assert_eq!(record.cost_usd, Some(0.0000555));
    assert_eq!((record.retries, record.error_class.clone()), (0, None));
    assert!(record.timestamp_ms > 1_700_000_000_000);
    // A key, a header, audio or the transcript is never in the file.
    let file = std::fs::read_to_string(&log).unwrap();
    for secret in ["or-secret-key-123", "Bearer", "UklGR", "She taught"] {
        assert!(!file.contains(secret), "a fixture secret reached the request log");
    }
}

#[test]
fn a_failed_request_is_logged_with_its_class_and_status() {
    let log = temp_log("failed");
    let (url, _seen) = one(401, vec![("X-Generation-Id", "gen-stt-bad")], "{}");
    let mut call = stt_call(&url, MAI);
    call.request_log = Some(&log);
    assert!(run_at(&call, &speech(), None).is_err());
    let record = &request_log::read(&log)[0];
    assert!(!record.ok());
    assert_eq!(record.error_class.as_deref(), Some("key_rejected"));
    assert_eq!(record.status, Some(401));
    assert_eq!(record.generation_id.as_deref(), Some("gen-stt-bad"));
    assert_eq!(record.cost_usd, None);

    // No server: no status, an unreachable class.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}/x");
    let mut call = stt_call(&url, MAI);
    call.request_log = Some(&log);
    assert!(run_at(&call, &speech(), None).is_err());
    let record = &request_log::read(&log)[1];
    assert_eq!((record.status, record.error_class.as_deref()), (None, Some("unreachable")));
}

#[test]
fn a_request_that_was_never_sent_is_not_logged() {
    let log = temp_log("unsent");
    let mut call = stt_call("http://127.0.0.1:9/x", MAI);
    call.key = None;
    call.request_log = Some(&log);
    assert!(run_at(&call, &speech(), None).unwrap_err().contains("no OpenRouter key"));
    let too_long = vec![0.0; 12_600_000];
    call.key = Some("k");
    assert!(run_at(&call, &too_long, None).is_err());
    assert!(request_log::read(&log).is_empty(), "no request, no record");
}

#[test]
fn the_chat_path_is_logged_as_chat_and_other_providers_as_transcription() {
    let log = temp_log("kinds");
    let reply = r#"{"choices":[{"message":{"content":"one two"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}"#;
    let (url, _seen) = one(200, vec![("x-generation-id", "gen-chat-1")], reply);
    let mut call = Call::new(Provider::OpenRouter, &url, "google/gemini-2.5-flash");
    call.key = Some("k");
    call.request_log = Some(&log);
    run_at(&call, &speech(), None).unwrap();
    let (url, _seen) = one(200, vec![], r#"{"text":"hi"}"#);
    let mut call = Call::new(Provider::Groq, &url, "whisper-large-v3-turbo");
    call.key = Some("k");
    call.request_log = Some(&log);
    run_at(&call, &speech(), None).unwrap();
    let records = request_log::read(&log);
    assert_eq!(records[0].endpoint_kind, "chat");
    assert_eq!(records[0].generation_id.as_deref(), Some("gen-chat-1"));
    assert_eq!((records[1].provider.as_str(), records[1].endpoint_kind.as_str()), ("groq", "transcription"));
}

fn catalogue_reply() -> Value {
    json!({ "data": [
        { "id": MAI, "name": "Microsoft AI: MAI-Transcribe 2",
          "architecture": { "input_modalities": ["audio"], "output_modalities": ["transcription"] },
          "pricing": { "prompt": "0.1", "completion": "0" } },
        { "id": "assemblyai/universal-3-6-pro", "name": "AssemblyAI: Universal-3.6 Pro",
          "architecture": { "input_modalities": ["audio"], "output_modalities": ["transcription"] },
          "pricing": { "prompt": "0.0000833333", "completion": "0" } },
        { "id": "google/gemini-3.5-transcribe", "name": "Google: Gemini 3.5 Transcribe",
          "architecture": { "input_modalities": ["audio"], "output_modalities": ["transcription"] },
          "pricing": { "prompt": "0.000002", "completion": "0.000012" } },
        { "id": "google/gemini-2.5-flash", "name": "Google: Gemini 2.5 Flash",
          "architecture": { "input_modalities": ["text", "audio"], "output_modalities": ["text"] },
          "pricing": { "prompt": "0.0000003", "completion": "0.0000025", "audio": "0.000001" } },
        { "id": "acme/text-only", "name": "Text only",
          "architecture": { "input_modalities": ["text"], "output_modalities": ["text"] },
          "pricing": { "prompt": "0.1", "completion": "0.1" } },
    ]})
}

#[test]
fn the_catalogue_marks_transcription_models_and_prices_them_per_audio_hour() {
    let models = openrouter::parse_catalogue(&catalogue_reply());
    let by_id = |id: &str| models.iter().find(|m| m.id == id).unwrap();
    assert!(models.iter().all(|m| m.id != "acme/text-only"));
    let mai = by_id(MAI);
    assert_eq!(mai.kind, openrouter::Kind::Transcription);
    // OpenRouter quotes MAI per hour ...
    assert!((mai.usd_per_hour.unwrap() - 0.10).abs() < 1e-12);
    // ... and most models per second: $0.0000833/s is $0.30/h.
    let assembly = by_id("assemblyai/universal-3-6-pro");
    assert!((assembly.usd_per_hour.unwrap() - 0.30).abs() < 1e-6);
    // A model billed by token has no hourly price to show.
    assert_eq!(by_id("google/gemini-3.5-transcribe").usd_per_hour, None);
    let chat = by_id("google/gemini-2.5-flash");
    assert_eq!((chat.kind, chat.usd_per_hour), (openrouter::Kind::Chat, None));
    assert_eq!(openrouter::hourly_price(0.0, 0.0), None);
    assert_eq!(openrouter::transcription_cost(mai, 1800.0), Some(0.05));
    assert_eq!(openrouter::transcription_cost(chat, 60.0), None);
}

#[test]
fn a_model_from_the_transcription_listing_is_a_transcription_model() {
    // The shape of `?output_modalities=transcription`: no text output at all.
    let listing = json!({ "data": [{
        "id": MAI,
        "architecture": { "modality": "audio->transcription",
            "input_modalities": ["audio"], "output_modalities": ["transcription"] },
        "pricing": { "prompt": "0.1", "completion": "0" },
    }]});
    let models = openrouter::parse_catalogue(&listing);
    assert_eq!(models[0].kind, openrouter::Kind::Transcription);
}

#[test]
fn the_bundled_catalogue_lists_mai_at_ten_cents_an_hour_and_the_table_agrees() {
    let bundled = openrouter::bundled();
    let mai = bundled.iter().find(|m| m.id == MAI).unwrap();
    assert_eq!(mai.kind, openrouter::Kind::Transcription);
    assert_eq!(mai.usd_per_hour, Some(0.10));
    assert_eq!(usd_per_hour(Provider::OpenRouter, MAI), Some(0.10));
    assert!((estimate_cost("openrouter", MAI, 3600.0).unwrap() - 0.10).abs() < 1e-12);
    assert!((estimate_cost("openrouter", MAI, 5.0).unwrap() - 0.000139).abs() < 1e-6);
    assert_eq!(openrouter::MEETING_MODEL, MAI);
    assert!(bundled.iter().any(|m| m.kind == openrouter::Kind::Chat));
}

#[test]
fn a_cache_without_transcription_models_is_fetched_again_at_once() {
    let path = temp_log("cache").with_file_name("openrouter-models.json");
    let chat_only = openrouter::parse_catalogue(&catalogue_reply())
        .into_iter()
        .filter(|m| m.kind == openrouter::Kind::Chat)
        .collect::<Vec<_>>();
    let old = serde_json::json!({ "fetched_at_ms": 1_000, "models": chat_only });
    std::fs::write(&path, old.to_string()).unwrap();
    // An hour old, but from before transcription models were listed.
    let mut fetched = false;
    let (models, source) = openrouter::load(&path, 1_000 + 3_600_000, || {
        fetched = true;
        Ok(openrouter::parse_catalogue(&catalogue_reply()))
    });
    assert!(fetched && source == openrouter::Source::Fetched);
    assert!(models.iter().any(|m| m.id == MAI));
    // Now it holds some, and an hour later it is fresh: no fetch.
    let (_, source) = openrouter::load(&path, 1_000 + 7_200_000, || {
        panic!("a fresh cache is not fetched")
    });
    assert_eq!(source, openrouter::Source::Cached);
    // An old cache file (no `kind`) still reads, as chat models.
    let legacy = r#"{"fetched_at_ms":5,"models":[{"id":"a/b","name":"B","prompt_usd":1.0,"completion_usd":2.0,"audio_usd":null}]}"#;
    std::fs::write(&path, legacy).unwrap();
    let models = openrouter::cached_or_bundled(&path);
    assert_eq!(models[0].kind, openrouter::Kind::Chat);
}

#[test]
fn the_endpoint_of_a_transcription_model_is_its_own_not_chat() {
    assert_eq!(
        openrouter::TRANSCRIPTION_URL,
        "https://openrouter.ai/api/v1/audio/transcriptions"
    );
    assert_ne!(openrouter::TRANSCRIPTION_URL, openrouter::CHAT_URL);
    assert!(openrouter::TRANSCRIPTION_MODELS_URL.ends_with("output_modalities=transcription"));
    // The fake catalogue is what a test thread reads.
    force_catalogue(openrouter::parse_catalogue(&catalogue_reply()));
    let ids: Vec<_> = transcription_models().into_iter().map(|m| m.id).collect();
    assert_eq!(
        ids,
        ["assemblyai/universal-3-6-pro", "google/gemini-3.5-transcribe", MAI]
    );
}

#[test]
fn the_meeting_engine_is_the_local_model_unless_a_cloud_one_is_chosen() {
    assert_eq!(route_meeting(None, true), None, "unset stays local, even with a Groq key");
    assert_eq!(route_meeting(Some(EngineChoice::Local), true), None);
    assert_eq!(route_meeting(Some(EngineChoice::Auto), true), Some(Provider::Groq));
    assert_eq!(route_meeting(Some(EngineChoice::Auto), false), None);
    assert_eq!(
        route_meeting(Some(EngineChoice::Openrouter), false),
        Some(Provider::OpenRouter)
    );
    // The model: the setting, else MAI at OpenRouter, else the default.
    assert_eq!(meeting_model_for(Provider::OpenRouter, None), MAI);
    assert_eq!(
        meeting_model_for(Provider::OpenRouter, Some("openai/whisper-large-v3")),
        "openai/whisper-large-v3"
    );
    assert_eq!(meeting_model_for(Provider::OpenRouter, Some("whisper-1")), MAI);
    assert_eq!(meeting_model_for(Provider::Groq, Some("whisper-large-v3")), "whisper-large-v3");
    assert_eq!(meeting_model_for(Provider::Groq, None), "whisper-large-v3-turbo");
}
