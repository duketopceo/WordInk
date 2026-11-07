//! "Connect OpenRouter": the button in Settings, onboarding's cloud step and
//! the meeting-engine offer. The browser sign-in runs on its own thread
//! (`openrouter_oauth::connect`); the key goes to the key store and is never
//! held here.

use std::sync::mpsc::{Receiver, TryRecvError};

use rune::{El, Tx};

use super::Ui;
use super::controls::{ButtonKind, Paint, button, icon_row};
use super::icons::WAVE;
use crate::desktop::i18n::{tr, tr_fill};
use crate::transcription::cloud::{self, EngineChoice, Provider};

/// The model the meeting offer selects once OpenRouter is connected.
pub(super) use crate::hardware::MEETING_OFFER_MODEL as MEETING_MODEL;

#[derive(Default)]
pub(super) struct Connect {
    waiting: Option<Receiver<Result<(), String>>>,
    pub(super) error: Option<String>,
    /// What the window last did, until the key cache catches up.
    connected: Option<bool>,
    /// Set when this window just connected, so the offer applies once.
    just_connected: bool,
}

impl Connect {
    pub(super) fn is_waiting(&self) -> bool {
        self.waiting.is_some()
    }

    pub(super) fn connected(&self) -> bool {
        self.connected
            .unwrap_or_else(|| cloud::has_key(Provider::OpenRouter))
    }
}

pub(super) fn start(ui: &mut Ui, _: &mut Tx) {
    if ui.openrouter.is_waiting() {
        return;
    }
    ui.openrouter.error = None;
    if !ui.live() {
        return;
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("openrouter-connect".into())
        .spawn(move || {
            let result = crate::openrouter_oauth::connect(crate::keys::store)
                .map(|_| ())
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
        });
    match spawned {
        Ok(_) => ui.openrouter.waiting = Some(receiver),
        Err(error) => ui.openrouter.error = Some(error.to_string()),
    }
}

pub(super) fn disconnect(ui: &mut Ui, _: &mut Tx) {
    if ui.live() {
        crate::openrouter_oauth::disconnect();
    }
    ui.openrouter.connected = Some(false);
    // Meetings that used OpenRouter fall back to the local chain.
    if ui.settings.meeting_engine == Some(EngineChoice::Openrouter) {
        ui.update(|settings| {
            settings.meeting_engine = None;
            settings.meeting_engine_model = None;
        });
    }
}

/// Applies a finished sign-in. Returns whether anything changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    // A meeting that fell back from the cloud to the local model says so.
    let mut changed = false;
    if let Some(note) = crate::meeting::take_fallback_note() {
        ui.meetings.sidebar_note = Some(note);
        changed = true;
    }
    changed | poll_sign_in(ui)
}

fn poll_sign_in(ui: &mut Ui) -> bool {
    let Some(receiver) = &ui.openrouter.waiting else {
        return false;
    };
    let result = match receiver.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return false,
        Err(TryRecvError::Disconnected) => Err("sign-in stopped".to_owned()),
    };
    ui.openrouter.waiting = None;
    match result {
        Ok(()) => {
            ui.openrouter.connected = Some(true);
            ui.openrouter.just_connected = true;
            offer_meeting_engine(ui);
            // On the first run's cloud step, the new key is the cloud engine.
            let at_engine = ui
                .tour
                .as_ref()
                .is_some_and(|tour| tour.flow.step() == crate::desktop::onboarding::Step::Engine);
            if at_engine {
                ui.update(|settings| settings.engine = EngineChoice::Openrouter);
                super::onboarding::input(ui, crate::desktop::onboarding::Input::KeyStored);
            }
        }
        Err(error) => ui.openrouter.error = Some(error),
    }
    true
}

/// Once connected, a meeting engine nobody chose becomes Nemotron 3.5
/// Transcribe on OpenRouter (an existing choice is never overridden).
pub(super) fn offer_meeting_engine(ui: &mut Ui) {
    if ui.settings.meeting_engine.is_none() {
        ui.update(|settings| {
            settings.meeting_engine = Some(EngineChoice::Openrouter);
            settings.meeting_engine_model = Some(MEETING_MODEL.to_owned());
        });
    }
}

/// The "Connect OpenRouter" button, or Connected with Disconnect.
pub(super) fn control(ui: &Ui, p: Paint) -> El {
    if ui.openrouter.is_waiting() {
        return button(
            p,
            "connect-openrouter",
            tr("Waiting for the browser…"),
            ButtonKind::Secondary,
            |_, _| {},
        );
    }
    if ui.openrouter.connected() {
        return rune::row()
            .key("openrouter-connected")
            .gap(8.0)
            .center_y()
            .child(
                rune::text(tr("Connected"))
                    .key("connected-label")
                    .font(p.k.ty.ui)
                    .color(p.muted()),
            )
            .child(button(
                p,
                "disconnect-openrouter",
                tr("Disconnect"),
                ButtonKind::Secondary,
                disconnect,
            ));
    }
    button(
        p,
        "connect-openrouter",
        tr("Connect OpenRouter"),
        ButtonKind::Primary,
        start,
    )
}

/// The Settings row: sign in instead of pasting a key.
pub(super) fn settings_row(ui: &Ui, p: Paint) -> El {
    let note = match &ui.openrouter.error {
        Some(error) => tr_fill("Could not connect: {}", error),
        None => tr("Signs in at openrouter.ai; the key is kept in your OS keyring").to_owned(),
    };
    icon_row(
        p,
        "openrouter-connect",
        &WAVE,
        tr("OpenRouter"),
        Some(&note),
        control(ui, p),
    )
}

/// The meeting-engine offer: never forced, with the trade-off in a line.
pub(super) fn meeting_offer_row(ui: &Ui, p: Paint) -> El {
    let connected = ui.openrouter.connected();
    let note = if connected {
        tr(
            "Best meeting accuracy: Nemotron 3.5 Transcribe on OpenRouter, about $0.12 per hour of meeting",
        )
    } else {
        tr(
            "Best meeting accuracy: connect OpenRouter (about $0.12 per hour of meeting). Without it, meetings stay on this computer: free and private, a little less accurate",
        )
    };
    let control = if connected {
        button(
            p,
            "use-nemotron",
            tr("Use it"),
            ButtonKind::Primary,
            |ui, _| {
                ui.update(|settings| {
                    settings.meeting_engine = Some(EngineChoice::Openrouter);
                    settings.meeting_engine_model = Some(MEETING_MODEL.to_owned());
                });
            },
        )
    } else {
        control(ui, p)
    };
    icon_row(
        p,
        "meeting-offer",
        &WAVE,
        tr("Meeting accuracy"),
        Some(note),
        control,
    )
}

/// Onboarding's cloud step: the same sign-in beside the key field.
pub(super) fn onboarding_row(ui: &Ui, p: Paint) -> El {
    rune::row()
        .key("openrouter-row")
        .gap(8.0)
        .center_y()
        .child(
            rune::text(tr("Or use OpenRouter without copying a key:"))
                .key("openrouter-help")
                .font(super::controls::DESCRIPTION)
                .color(p.faint()),
        )
        .child(control(ui, p))
}

/// What the first meeting says while meetings stay on the local chain.
pub(super) fn first_meeting_note() -> String {
    tr("Meetings stay on this computer. For the best accuracy, connect OpenRouter in Meetings settings.")
        .to_owned()
}
