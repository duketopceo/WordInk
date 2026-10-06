//! WebAssembly bindings for `wordink-core`, consumed by `@wordink/core`.
//!
//! The boundary is deliberately narrow. Events go in through one method per
//! event kind with plain arguments, so nothing has to be parsed here. Each
//! call returns its effects as a JSON array (hand-written, to keep the wasm
//! small, KTD1). Binary payloads never go through JSON: an effect names a
//! blob slot, and the host takes it with [`WasmSession::take_bytes`] or
//! [`WasmSession::take_pcm`] (a `Uint8Array` / `Int16Array`) before its next
//! call into the session, which clears the slots.

use wasm_bindgen::prelude::*;
use wordink_core::providers::{Capabilities, Deepgram, Groq, HostProvider, OpenAi, Provider};
use wordink_core::{
    Effect, ErrorCode, Event, HostResult, HttpBody, Mode, PartValue, Session, SessionConfig, State,
    WsData,
};

/// The close code a host reports for a socket that timed out before it
/// opened; the core maps it to `ProviderDown`, every other pre-open code to
/// `AuthFailed` (`wordink_core::providers::WS_CLOSE_STALL`). Exported as a
/// getter: wasm-bindgen does not export constants.
#[wasm_bindgen]
pub fn ws_connect_stall_code() -> u16 {
    wordink_core::providers::WS_CLOSE_STALL
}

/// A binary payload waiting for the host.
#[derive(Debug)]
enum Blob {
    Bytes(Vec<u8>),
    Pcm(Vec<i16>),
}

/// A dictation session for one provider configuration.
#[wasm_bindgen]
#[derive(Debug)]
pub struct WasmSession {
    session: Session,
    blobs: Vec<Blob>,
}

#[wasm_bindgen]
impl WasmSession {
    /// Creates an idle session.
    ///
    /// `kind` is `groq-relay` (credential = relay endpoint), `groq-key`
    /// (credential = dev key), `openai` (credential = `ek_` secret or dev
    /// key), `deepgram` (credential = JWT or dev key) or `host` (credential =
    /// host provider id, with `streaming` and `sample_rate` its declared
    /// capabilities). Empty `model` / `language` / `hint` mean "default".
    #[wasm_bindgen(constructor)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: &str,
        toggle: bool,
        credential: String,
        model: String,
        language: String,
        hint: String,
        streaming: bool,
        sample_rate: u32,
    ) -> Result<WasmSession, JsError> {
        Self::create(
            kind,
            toggle,
            credential,
            model,
            language,
            hint,
            streaming,
            sample_rate,
        )
        .map_err(JsError::new)
    }

    /// Current state name (see the `state` effect).
    pub fn state(&self) -> String {
        state_name(self.session.state()).into()
    }

    /// The dictation button went down.
    pub fn press(&mut self) -> String {
        self.handle(Event::Press)
    }

    /// The dictation button came up.
    pub fn release(&mut self) -> String {
        self.handle(Event::Release)
    }

    /// Microphone capture started at `sample_rate` Hz.
    pub fn mic_granted(&mut self, sample_rate: u32) -> String {
        self.handle(Event::MicGranted { sample_rate })
    }

    /// Microphone access was refused or failed.
    pub fn mic_denied(&mut self) -> String {
        self.handle(Event::MicDenied)
    }

    /// An HTTP response arrived.
    pub fn http_response(&mut self, id: u32, status: u16, body: String) -> String {
        self.handle(Event::HttpResponse { id, status, body })
    }

    /// An HTTP request got no response.
    pub fn http_failed(&mut self, id: u32) -> String {
        self.handle(Event::HttpFailed { id })
    }

    /// A socket opened.
    pub fn ws_opened(&mut self, id: u32) -> String {
        self.handle(Event::WsOpened { id })
    }

    /// A socket received a text message.
    pub fn ws_text(&mut self, id: u32, text: String) -> String {
        self.handle(Event::WsMessage {
            id,
            data: WsData::Text(text),
        })
    }

    /// A socket received a binary message.
    pub fn ws_binary(&mut self, id: u32, data: Vec<u8>) -> String {
        self.handle(Event::WsMessage {
            id,
            data: WsData::Binary(data),
        })
    }

    /// A socket closed or failed to open.
    pub fn ws_closed(&mut self, id: u32, code: u16) -> String {
        self.handle(Event::WsClosed { id, code })
    }

    /// The host provider reported interim text.
    pub fn host_interim(&mut self, text: String) -> String {
        self.handle(Event::HostProviderResult(HostResult::Interim { text }))
    }

    /// The host provider reported its final text.
    pub fn host_final(&mut self, text: String) -> String {
        self.handle(Event::HostProviderResult(HostResult::Final { text }))
    }

    /// The host provider failed with an error code name (unknown names map
    /// to `ProviderDown`).
    pub fn host_error(&mut self, code: &str) -> String {
        let code = parse_code(code);
        self.handle(Event::HostProviderResult(HostResult::Error { code }))
    }

    /// Feeds captured mono Float32 audio at the granted rate.
    pub fn push_audio(&mut self, samples: &[f32]) -> String {
        self.blobs.clear();
        let fx = self.session.push_audio(samples);
        self.encode(&fx)
    }

    /// Takes a byte blob named by the last call's effects.
    pub fn take_bytes(&mut self, slot: usize) -> Vec<u8> {
        match self.blobs.get_mut(slot) {
            Some(Blob::Bytes(b)) => core::mem::take(b),
            _ => Vec::new(),
        }
    }

    /// Takes a PCM16 blob named by the last call's effects.
    pub fn take_pcm(&mut self, slot: usize) -> Vec<i16> {
        match self.blobs.get_mut(slot) {
            Some(Blob::Pcm(p)) => core::mem::take(p),
            _ => Vec::new(),
        }
    }
}

impl WasmSession {
    /// [`WasmSession::new`] without the JS error type, for native tests.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        kind: &str,
        toggle: bool,
        credential: String,
        model: String,
        language: String,
        hint: String,
        streaming: bool,
        sample_rate: u32,
    ) -> Result<WasmSession, &'static str> {
        let non_empty = |s: String| (!s.is_empty()).then_some(s);
        let (model, language) = (non_empty(model), non_empty(language));
        if credential.is_empty() {
            return Err("missing credential");
        }
        let provider = match kind {
            "groq-relay" | "groq-key" => {
                let mut g = if kind == "groq-relay" {
                    Groq::relay(credential)
                } else {
                    Groq::dev_key(credential)
                };
                if let Some(m) = model {
                    g.model = m;
                }
                g.language = language;
                Provider::Groq(g)
            }
            "openai" => {
                let mut o = OpenAi::new(credential);
                if let Some(m) = model {
                    o.model = m;
                }
                Provider::OpenAi(o)
            }
            "deepgram" => {
                let mut d = Deepgram::new(credential);
                if let Some(m) = model {
                    d.model = m;
                }
                Provider::Deepgram(d)
            }
            "host" => {
                if sample_rate == 0 {
                    return Err("host provider sample rate must be non-zero");
                }
                Provider::Host(HostProvider::new(
                    credential,
                    Capabilities {
                        streaming,
                        sample_rate,
                    },
                ))
            }
            _ => return Err("unknown provider kind"),
        };
        let mode = if toggle {
            Mode::Toggle
        } else {
            Mode::PushToTalk
        };
        let mut config = SessionConfig::new(mode, provider);
        config.hint = non_empty(hint);
        Ok(WasmSession {
            session: Session::new(config),
            blobs: Vec::new(),
        })
    }

    fn handle(&mut self, event: Event) -> String {
        self.blobs.clear();
        let fx = self.session.handle(event);
        self.encode(&fx)
    }

    fn blob(&mut self, blob: Blob) -> usize {
        self.blobs.push(blob);
        self.blobs.len() - 1
    }

    /// Effects as a JSON array; binary payloads move into blob slots.
    fn encode(&mut self, fx: &[Effect]) -> String {
        let mut out = String::from("[");
        for effect in fx {
            let start = out.len();
            if out.len() > 1 {
                out.push(',');
            }
            if !self.encode_one(effect, &mut out) {
                // An effect kind this host does not know yet: skip it.
                out.truncate(start);
            }
        }
        out.push(']');
        out
    }

    fn encode_one(&mut self, effect: &Effect, out: &mut String) -> bool {
        match effect {
            Effect::State(state) => {
                tag(out, "state");
                field(out, "s", state_name(*state));
                if let State::Error(code) = state {
                    field(out, "code", code_name(*code));
                }
            }
            Effect::RequestMic => tag(out, "request-mic"),
            Effect::StopMic => tag(out, "stop-mic"),
            Effect::Level { rms } => {
                tag(out, "level");
                // Millionths, as an integer: float formatting costs wasm size.
                let micro = (rms.clamp(0.0, 1.0) * 1_000_000.0) as u32;
                num(out, "rms", micro);
            }
            Effect::Interim { text } => {
                tag(out, "interim");
                field(out, "text", text);
            }
            Effect::Final { text } => {
                tag(out, "final");
                field(out, "text", text);
            }
            Effect::HttpRequest(req) => {
                tag(out, "http");
                num(out, "id", req.id);
                field(out, "method", &req.method);
                field(out, "url", &req.url);
                out.push_str(",\"headers\":[");
                for (i, (k, v)) in req.headers.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('[');
                    quote(k, out);
                    out.push(',');
                    quote(v, out);
                    out.push(']');
                }
                out.push_str("],\"parts\":[");
                let HttpBody::Multipart(parts) = &req.body else {
                    return false;
                };
                for (i, part) in parts.iter().enumerate() {
                    out.push_str(if i > 0 { ",{\"name\":" } else { "{\"name\":" });
                    quote(&part.name, out);
                    match &part.value {
                        PartValue::Text(text) => field(out, "text", text),
                        PartValue::File {
                            filename,
                            content_type,
                            data,
                        } => {
                            field(out, "filename", filename);
                            field(out, "type", content_type);
                            let slot = self.blob(Blob::Bytes(data.clone()));
                            num(out, "blob", slot as u32);
                        }
                    }
                    out.push('}');
                }
                out.push(']');
            }
            Effect::WsOpen { id, url, protocols } => {
                tag(out, "ws-open");
                num(out, "id", *id);
                field(out, "url", url);
                out.push_str(",\"protocols\":[");
                for (i, p) in protocols.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    quote(p, out);
                }
                out.push(']');
            }
            Effect::WsSend { id, data } => {
                tag(out, "ws-send");
                num(out, "id", *id);
                match data {
                    WsData::Text(text) => field(out, "text", text),
                    WsData::Binary(bytes) => {
                        let slot = self.blob(Blob::Bytes(bytes.clone()));
                        num(out, "blob", slot as u32);
                    }
                }
            }
            Effect::WsClose { id } => {
                tag(out, "ws-close");
                num(out, "id", *id);
            }
            Effect::HostProviderStart {
                id,
                sample_rate,
                hint,
            } => {
                tag(out, "hp-start");
                field(out, "id", id);
                num(out, "rate", *sample_rate);
                if let Some(hint) = hint {
                    field(out, "hint", hint);
                }
            }
            Effect::HostProviderAudio { id, samples } => {
                tag(out, "hp-audio");
                field(out, "id", id);
                let slot = self.blob(Blob::Pcm(samples.clone()));
                num(out, "pcm", slot as u32);
            }
            Effect::HostProviderFinish { id } => {
                tag(out, "hp-finish");
                field(out, "id", id);
            }
            Effect::HostProviderCancel { id } => {
                tag(out, "hp-cancel");
                field(out, "id", id);
            }
            _ => return false,
        }
        out.push('}');
        true
    }
}

fn state_name(state: State) -> &'static str {
    match state {
        State::Idle => "idle",
        State::RequestingMic => "requesting-mic",
        State::Listening => "listening",
        State::Transcribing => "transcribing",
        State::Error(_) => "error",
    }
}

const CODES: [(ErrorCode, &str); 6] = [
    (ErrorCode::MicDenied, "MicDenied"),
    (ErrorCode::NoSpeech, "NoSpeech"),
    (ErrorCode::AuthFailed, "AuthFailed"),
    (ErrorCode::RateLimited, "RateLimited"),
    (ErrorCode::ProviderDown, "ProviderDown"),
    (ErrorCode::BadAudio, "BadAudio"),
];

fn code_name(code: ErrorCode) -> &'static str {
    CODES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or("ProviderDown", |(_, n)| n)
}

fn parse_code(name: &str) -> ErrorCode {
    CODES
        .iter()
        .find(|(_, n)| *n == name)
        .map_or(ErrorCode::ProviderDown, |(c, _)| *c)
}

/// Opens an effect object: `{"t":"<name>"`.
fn tag(out: &mut String, name: &str) {
    out.push_str("{\"t\":\"");
    out.push_str(name);
    out.push('"');
}

fn field(out: &mut String, key: &str, value: &str) {
    out.push_str(",\"");
    out.push_str(key);
    out.push_str("\":");
    quote(value, out);
}

fn num(out: &mut String, key: &str, value: u32) {
    out.push_str(",\"");
    out.push_str(key);
    out.push_str("\":");
    let mut digits = [0u8; 10];
    let mut n = value;
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for &d in &digits[i..] {
        out.push(d as char);
    }
}

/// Writes `s` as a JSON string literal.
fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.push_str("\\u00");
                out.push(HEX[(c as usize) >> 4] as char);
                out.push(HEX[(c as usize) & 15] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(kind: &str, credential: &str) -> WasmSession {
        WasmSession::create(
            kind,
            false,
            credential.into(),
            String::new(),
            String::new(),
            String::new(),
            false,
            0,
        )
        .unwrap()
    }

    /// 1 kHz tone at 48 kHz, loud enough to count as speech.
    fn tone(seconds: f32) -> Vec<f32> {
        let n = (48_000.0 * seconds) as usize;
        (0..n)
            .map(|i| 0.3 * (i as f32 * 2.0 * core::f32::consts::PI * 1000.0 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn press_encodes_state_then_request_mic() {
        let mut s = session("groq-relay", "https://relay.test/api");
        assert_eq!(
            s.press(),
            r#"[{"t":"state","s":"requesting-mic"},{"t":"request-mic"}]"#
        );
        assert_eq!(s.state(), "requesting-mic");
    }

    #[test]
    fn mic_denied_encodes_error_code() {
        let mut s = session("groq-relay", "https://relay.test/api");
        s.press();
        assert_eq!(
            s.mic_denied(),
            r#"[{"t":"state","s":"error","code":"MicDenied"}]"#
        );
    }

    #[test]
    fn groq_cycle_moves_wav_into_a_blob_slot() {
        let mut s = session("groq-relay", "https://relay.test/api/");
        s.press();
        assert_eq!(s.mic_granted(48_000), r#"[{"t":"state","s":"listening"}]"#);
        let levels = s.push_audio(&tone(0.5));
        assert!(
            levels.starts_with(r#"[{"t":"level","rms":2121"#),
            "{levels}"
        );
        let fx = s.release();
        assert_eq!(
            fx,
            concat!(
                r#"[{"t":"stop-mic"},{"t":"state","s":"transcribing"},"#,
                r#"{"t":"http","id":1,"method":"POST","url":"https://relay.test/api/groq/transcriptions","#,
                r#""headers":[],"parts":[{"name":"file","filename":"audio.wav","type":"audio/wav","blob":0},"#,
                r#"{"name":"model","text":"whisper-large-v3-turbo"},{"name":"response_format","text":"text"}]}]"#
            )
        );
        let wav = s.take_bytes(0);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 8_000 * 2);
        assert!(s.take_bytes(0).is_empty(), "a slot is taken once");
        assert_eq!(
            s.http_response(1, 200, "hello there\n".into()),
            r#"[{"t":"final","text":"hello there"},{"t":"state","s":"idle"}]"#
        );
    }

    #[test]
    fn dev_key_sets_authorization_header_and_options_apply() {
        let mut s = WasmSession::create(
            "groq-key",
            true,
            "gsk_dev".into(),
            "whisper-large-v3".into(),
            "en".into(),
            "WordInk".into(),
            false,
            0,
        )
        .unwrap();
        s.press();
        s.mic_granted(48_000);
        s.push_audio(&tone(0.4));
        s.release();
        let fx = s.press();
        assert!(
            fx.contains(r#""headers":[["Authorization","Bearer gsk_dev"]]"#),
            "{fx}"
        );
        assert!(fx.contains(r#"{"name":"model","text":"whisper-large-v3"}"#));
        assert!(fx.contains(r#"{"name":"prompt","text":"WordInk"}"#));
        assert!(fx.contains(r#"{"name":"language","text":"en"}"#));
    }

    #[test]
    fn host_provider_audio_moves_into_a_pcm_slot() {
        let mut s = WasmSession::create(
            "host",
            false,
            "local".into(),
            "".into(),
            "".into(),
            "".into(),
            true,
            16_000,
        )
        .unwrap();
        s.press();
        assert_eq!(
            s.mic_granted(48_000),
            r#"[{"t":"state","s":"listening"},{"t":"hp-start","id":"local","rate":16000}]"#
        );
        let fx = s.push_audio(&tone(0.1));
        let at = fx.find(r#"{"t":"hp-audio","id":"local","pcm":0}"#);
        assert!(at.is_some(), "{fx}");
        let pcm = s.take_pcm(0);
        assert!((1_560..=1_600).contains(&pcm.len()), "{}", pcm.len());
        s.push_audio(&tone(0.3));
        s.release();
        assert_eq!(
            s.host_final("hi".into()),
            r#"[{"t":"final","text":"hi"},{"t":"state","s":"idle"}]"#
        );
    }

    #[test]
    fn host_error_maps_names_and_defaults_to_provider_down() {
        let mut s = WasmSession::create(
            "host",
            false,
            "x".into(),
            "".into(),
            "".into(),
            "".into(),
            true,
            16_000,
        )
        .unwrap();
        s.press();
        s.mic_granted(16_000);
        assert_eq!(
            s.host_error("RateLimited"),
            r#"[{"t":"stop-mic"},{"t":"state","s":"error","code":"RateLimited"}]"#
        );
        s.press();
        s.mic_granted(16_000);
        assert!(s.host_error("Bogus").contains(r#""code":"ProviderDown""#));
    }

    #[test]
    fn deepgram_opens_socket_with_token_subprotocol() {
        let mut s = session("deepgram", "jwt.abc");
        s.press();
        let fx = s.mic_granted(48_000);
        assert!(
            fx.contains(r#"{"t":"ws-open","id":1,"url":"wss://"#),
            "{fx}"
        );
        assert!(fx.contains(r#""protocols":["token","jwt.abc"]}"#), "{fx}");
        s.ws_opened(1);
        let fx = s.push_audio(&tone(0.1));
        assert!(fx.contains(r#"{"t":"ws-send","id":1,"blob":0}"#), "{fx}");
        assert_eq!(s.take_bytes(0).len() % 2, 0);
    }

    #[test]
    fn rejects_bad_config() {
        let bad = |kind: &str, cred: &str, rate: u32| {
            WasmSession::create(
                kind,
                false,
                cred.into(),
                "".into(),
                "".into(),
                "".into(),
                false,
                rate,
            )
            .unwrap_err()
        };
        assert_eq!(bad("whisper", "k", 0), "unknown provider kind");
        assert_eq!(bad("groq-key", "", 0), "missing credential");
        assert_eq!(
            bad("host", "id", 0),
            "host provider sample rate must be non-zero"
        );
    }

    #[test]
    fn quote_escapes_json() {
        let mut out = String::new();
        quote("a\"b\\c\nd\u{1}é", &mut out);
        assert_eq!(out, r#""a\"b\\c\nd\u0001é""#);
        let mut n = String::new();
        num(&mut n, "k", 0);
        num(&mut n, "k", u32::MAX);
        assert_eq!(n, r#","k":0,"k":4294967295"#);
    }
}
