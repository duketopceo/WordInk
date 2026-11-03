//! Every failure a dictation can hit, as one coded report with a specific
//! reason and one concrete next step. Raising it is always loud: the error
//! tone, the HUD's failure state, a critical desktop notification, and the
//! event log (which `wordink diagnostics` reads as Recent errors) with the
//! code at the front, plus a debug-log entry carrying the same code.

/// What a failure shows and sounds through. The listener's is real; tests
/// record the calls.
pub(crate) trait Alerts {
    fn tone(&mut self);
    fn hud(&mut self, text: &str);
    fn notify(&mut self, text: &str);
    fn event(&mut self, text: &str) -> color_eyre::eyre::Result<()>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    /// The earbuds would not switch to headset mode.
    EarbudsProfile {
        device: String,
        detail: String,
        ms: Option<u64>,
    },
    /// They switched, but their microphone never appeared.
    EarbudsNoSource { device: String, ms: Option<u64> },
    /// The microphone exists but the recording stream would not link to it.
    StreamNotLinked { device: String, reason: String },
    /// Another program keeps taking the stream; the message names it.
    StreamHijacked { message: String },
    /// The chosen microphone is not connected.
    MicGone { device: String },
    /// The microphone delivered only zeros.
    Silent { device: String },
    /// The route was confirmed but the stream delivered no audio.
    NoAudio {
        device: String,
        held_ms: Option<u64>,
        since_confirm_ms: Option<u64>,
    },
    /// The shortcut came up before the microphone was ready.
    ReleasedEarly { device: String },
    /// Transcription failed (network, auth, quota, HTTP status).
    Engine { detail: String },
    /// The words were transcribed but could not be pasted.
    Paste { detail: String },
}

impl Failure {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::EarbudsProfile { .. } => "E-EARBUDS-PROFILE",
            Self::EarbudsNoSource { .. } => "E-EARBUDS-NOSOURCE",
            Self::StreamNotLinked { .. } => "E-STREAM-LINK",
            Self::StreamHijacked { .. } => "E-STREAM-HIJACKED",
            Self::MicGone { .. } => "E-MIC-GONE",
            Self::Silent { .. } => "E-MIC-SILENT",
            Self::NoAudio { .. } => "E-EARBUDS-NOAUDIO",
            Self::ReleasedEarly { .. } => "E-RELEASED-EARLY",
            Self::Engine { detail } => engine_code(detail),
            Self::Paste { .. } => "E-PASTE",
        }
    }

    /// The reason and the next step, without the code.
    pub(crate) fn message(&self) -> String {
        let after = |ms: &Option<u64>| ms.map_or(String::new(), |ms| format!(" after {ms} ms"));
        match self {
            Self::EarbudsProfile { device, detail, ms } => format!(
                "{device}: couldn't switch to headset mode{} ({detail}). Reconnect the earbuds and try again.",
                after(ms)
            ),
            Self::EarbudsNoSource { device, ms } => format!(
                "{device}: switched to headset mode but its microphone didn't appear{}. Reconnect the earbuds, or pick another microphone in Settings.",
                after(ms)
            ),
            Self::StreamNotLinked { device, reason } => format!(
                "{device}: its microphone exists but the recording stream wouldn't link ({reason}). Try again; if it repeats, restart PipeWire."
            ),
            Self::StreamHijacked { message } => message.clone(),
            Self::MicGone { device } => {
                format!(
                    "{device} isn't connected. Reconnect it or pick another microphone in Settings."
                )
            }
            Self::Silent { device } => format!(
                "{device} is silent \u{2014} check the microphone. Make sure it isn't muted."
            ),
            Self::NoAudio {
                device,
                held_ms,
                since_confirm_ms,
            } => {
                let timing = match (held_ms, since_confirm_ms) {
                    (Some(held), Some(confirm)) => {
                        format!(" (held {held} ms, {confirm} ms after the route was confirmed)")
                    }
                    _ => String::new(),
                };
                format!(
                    "{device} connected but sent no audio{timing} \u{2014} try again; if it repeats, reconnect the earbuds."
                )
            }
            Self::ReleasedEarly { device } => format!(
                "{device}: released before the microphone was ready \u{2014} hold a little longer; the earbuds take about 1 s to switch."
            ),
            Self::Engine { detail } => format!("{detail}. {}", engine_next_step(detail)),
            Self::Paste { detail } => format!(
                "Paste failed: {detail}. Press Ctrl+V if the text reached your clipboard, or check wtype and the RemoteDesktop portal."
            ),
        }
    }

    /// What the event log, HUD and notification carry: `[code] message`.
    pub(crate) fn text(&self) -> String {
        format!("[{}] {}", self.code(), self.message())
    }

    /// A failed earbuds flow, by which step went wrong.
    pub(crate) fn from_headset(device: &str, error: &str, ms: Option<u64>) -> Self {
        let device = device.to_owned();
        if error.contains("not connected") {
            Self::MicGone { device }
        } else if error.contains("did not appear") || error.contains("not in a mode") {
            Self::EarbudsNoSource { device, ms }
        } else {
            Self::EarbudsProfile {
                device,
                detail: error.to_owned(),
                ms,
            }
        }
    }

    /// A failed microphone open or an interrupted stream, by its reason.
    pub(crate) fn from_open(device: &str, error: &str) -> Self {
        let device = device.to_owned();
        if error.contains("capturing WordInk") || error.contains("keeps moving WordInk") {
            Self::StreamHijacked {
                message: error.to_owned(),
            }
        } else if error.contains("no source")
            || error.contains("not connected")
            || error.contains("device unavailable")
            || error.contains("unavailable")
        {
            Self::MicGone { device }
        } else {
            Self::StreamNotLinked {
                device,
                reason: error.to_owned(),
            }
        }
    }

    /// A failed transcription or paste, by where it failed.
    pub(crate) fn from_job(error: &str) -> Self {
        let lower = error.to_lowercase();
        if lower.contains("paste") || lower.contains("clipboard") || lower.contains("wtype") {
            Self::Paste {
                detail: error.to_owned(),
            }
        } else {
            Self::Engine {
                detail: error.to_owned(),
            }
        }
    }
}

fn engine_code(detail: &str) -> &'static str {
    let lower = detail.to_lowercase();
    if lower.contains("key") || lower.contains("401") || lower.contains("403") {
        "E-ENGINE-AUTH"
    } else if lower.contains("credit") || lower.contains("402") {
        "E-ENGINE-CREDIT"
    } else if lower.contains("rate") || lower.contains("429") {
        "E-ENGINE-RATE"
    } else if lower.contains("http")
        || lower.contains("network")
        || lower.contains("unavailable")
        || lower.contains("timed out")
    {
        "E-ENGINE-HTTP"
    } else {
        "E-ENGINE"
    }
}

fn engine_next_step(detail: &str) -> &'static str {
    match engine_code(detail) {
        "E-ENGINE-AUTH" => "Check the engine's API key in Settings.",
        "E-ENGINE-CREDIT" => "Add credit with your provider, or switch engines in Settings.",
        "E-ENGINE-RATE" => "Wait a moment and try again, or switch engines in Settings.",
        "E-ENGINE-HTTP" => "Check your connection and try again, or switch to a local model.",
        _ => "Try again, or check `wordink logs --debug`.",
    }
}

/// Raises `failure`: tone, HUD failure, critical notification, event log, and
/// a debug-log entry with its code.
pub(crate) fn raise(failure: &Failure, alerts: &mut impl Alerts) -> color_eyre::eyre::Result<()> {
    let text = failure.text();
    crate::debug_log::dlog!(
        Info,
        "app.error",
        "dictation failed",
        code = failure.code(),
        message = text,
    );
    alerts.tone();
    alerts.hud(&text);
    alerts.notify(&text);
    alerts.event(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorded {
        calls: Vec<&'static str>,
        text: String,
    }

    impl Alerts for Recorded {
        fn tone(&mut self) {
            self.calls.push("tone");
        }
        fn hud(&mut self, text: &str) {
            self.calls.push("hud");
            self.text = text.to_owned();
        }
        fn notify(&mut self, text: &str) {
            assert_eq!(text, self.text, "the HUD and the notification say the same");
            self.calls.push("notify");
        }
        fn event(&mut self, text: &str) -> color_eyre::eyre::Result<()> {
            assert_eq!(text, self.text);
            self.calls.push("event");
            Ok(())
        }
    }

    const DEVICE: &str = "Between 3ANC";

    fn every_failure() -> Vec<Failure> {
        vec![
            Failure::from_headset(
                DEVICE,
                "could not switch the earbuds to headset-head-unit",
                Some(258),
            ),
            Failure::from_headset(
                DEVICE,
                "the earbuds' microphone did not appear in headset-head-unit",
                Some(2_000),
            ),
            Failure::from_open(DEVICE, "no capture stream appeared in the sound server"),
            Failure::from_open(
                DEVICE,
                "EasyEffects is capturing WordInk's microphone \u{2014} exclude WordInk in EasyEffects \u{203a} Input",
            ),
            Failure::from_headset(DEVICE, "the earbuds are not connected", None),
            Failure::Silent {
                device: DEVICE.into(),
            },
            Failure::ReleasedEarly {
                device: DEVICE.into(),
            },
            Failure::NoAudio {
                device: DEVICE.into(),
                held_ms: Some(6_200),
                since_confirm_ms: Some(5_800),
            },
            Failure::from_job("Groq could not transcribe this clip (HTTP 503)"),
            Failure::from_job("Groq rejected the API key"),
            Failure::from_job("could not paste: wtype failed"),
        ]
    }

    #[test]
    fn every_failure_is_loud_coded_and_says_what_to_do() {
        let mut codes = std::collections::HashSet::new();
        for failure in every_failure() {
            let mut alerts = Recorded::default();
            raise(&failure, &mut alerts).unwrap();
            assert_eq!(
                alerts.calls,
                ["tone", "hud", "notify", "event"],
                "{failure:?}"
            );
            let text = alerts.text;
            assert!(
                text.starts_with(&format!("[{}] ", failure.code())),
                "{text}"
            );
            assert!(
                text.contains(". ") || text.contains('\u{2014}'),
                "a next step: {text}"
            );
            codes.insert(failure.code());
        }
        assert_eq!(codes.len(), 11, "each kind has its own code: {codes:?}");
    }

    #[test]
    fn earbud_failures_say_which_step_and_how_long_it_took() {
        let profile = Failure::from_headset(
            DEVICE,
            "could not switch the earbuds to headset-head-unit",
            Some(258),
        );
        assert_eq!(profile.code(), "E-EARBUDS-PROFILE");
        assert!(profile.message().contains("after 258 ms"));
        let source = Failure::from_headset(
            DEVICE,
            "the earbuds' microphone did not appear in x",
            Some(2_000),
        );
        assert_eq!(source.code(), "E-EARBUDS-NOSOURCE");
        assert!(source.message().contains("after 2000 ms"));
        let link = Failure::from_open(
            DEVICE,
            "the stream is not on the chosen source after the move",
        );
        assert_eq!(link.code(), "E-STREAM-LINK");
    }

    #[test]
    fn a_hijack_names_the_program_and_a_released_early_press_says_how_long_to_hold() {
        let hijack = Failure::from_open(
            DEVICE,
            "EasyEffects is capturing WordInk's microphone \u{2014} exclude WordInk in EasyEffects \u{203a} Input, or turn off Process All Inputs",
        );
        assert_eq!(hijack.code(), "E-STREAM-HIJACKED");
        assert!(hijack.message().starts_with("EasyEffects is capturing"));
        let early = Failure::ReleasedEarly {
            device: DEVICE.into(),
        };
        assert!(early.message().contains("hold a little longer"));
        assert!(early.message().contains("about 1 s"));
    }

    #[test]
    fn engine_errors_carry_the_provider_status_and_a_code_by_kind() {
        let http = Failure::from_job("Groq could not transcribe this clip (HTTP 503)");
        assert_eq!(http.code(), "E-ENGINE-HTTP");
        assert!(http.message().contains("HTTP 503") && http.message().contains("Groq"));
        assert_eq!(
            Failure::from_job("Groq rejected the API key").code(),
            "E-ENGINE-AUTH"
        );
        assert_eq!(
            Failure::from_job("rate limited (429)").code(),
            "E-ENGINE-RATE"
        );
        assert_eq!(
            Failure::from_job("out of credit (402)").code(),
            "E-ENGINE-CREDIT"
        );
        assert_eq!(Failure::from_job("something odd").code(), "E-ENGINE");
        assert_eq!(
            Failure::from_job("could not paste: no wtype").code(),
            "E-PASTE"
        );
    }

    #[test]
    fn a_disconnected_microphone_is_named() {
        let gone = Failure::from_open(DEVICE, "the sound server has no source bluez_input.X");
        assert_eq!(gone.code(), "E-MIC-GONE");
        assert!(gone.message().starts_with(DEVICE));
    }

    #[test]
    fn the_code_survives_into_diagnostics_recent_errors() {
        let text = Failure::Silent {
            device: DEVICE.into(),
        }
        .text();
        let scrubbed = crate::diagnostics::scrub(&text, &crate::diagnostics::Redactions::default());
        assert!(scrubbed.starts_with("[E-MIC-SILENT] "), "{scrubbed}");
    }
}
