//! The first run's state machine in the window: [`Tour`] feeds
//! `onboarding::Flow` the permissions and the model download, starts the
//! download, asks for permissions, relaunches when macOS needs it, watches
//! the practice dictation, and records completion. The first run the person
//! sees is the onboarding story over it ([`onboarding_kit::live`]), the one
//! onboarding on every OS.
//!
//! [`onboarding_kit::live`]: super::onboarding_kit::live

use std::time::{Duration, Instant};

use super::Request;
use super::Ui;
use super::platform::{PlatformRequest, PlatformState};
use crate::app_settings::AppSettings;
use crate::desktop::onboarding::{
    Effect, Flow, Input, ItemStatus, ModelProgress, Observation, PermissionState, SetupGap,
    SetupItem, Step,
};

/// How long the ready check shows before setup moves on.
const ADVANCE_BEAT: Duration = Duration::from_millis(700);

pub(super) struct Tour {
    pub(super) flow: Flow,
    advance_at: Option<Instant>,
    completed: bool,
    /// A Groq key was already set, and the engine isn't Local, when the tour
    /// opened: cloud dictation works without a local model.
    cloud_ready: bool,
    /// Run setup again: an existing install, whose settings stay as they are
    /// (the engine step still shows the recommendation).
    pub(super) replayed: bool,
}

/// Whether dictation already runs on the cloud: a key resolves (once, here,
/// since the keyring can be slow) and the engine setting doesn't say Local.
/// Tests never see this machine's keys.
fn key_already_set(settings: &AppSettings) -> bool {
    #[cfg(test)]
    {
        let _ = settings;
        false
    }
    #[cfg(not(test))]
    {
        use crate::transcription::cloud::{EngineChoice, Provider};
        settings.engine != EngineChoice::Local
            && crate::keys::status(Provider::Groq.id(), Provider::Groq.key_env()).is_some()
    }
}

impl Tour {
    fn new(flow: Flow, completed: bool, cloud_ready: bool) -> Self {
        Self {
            flow,
            advance_at: None,
            completed,
            cloud_ready,
            replayed: false,
        }
    }

    /// The tour from Welcome, as on a new install: Run setup again.
    pub(super) fn replay(
        settings: &AppSettings,
        platform: &PlatformState,
        picker: &super::picker::Picker,
    ) -> Tour {
        let cloud = key_already_set(settings);
        let installed = picker.selected_installed(settings) || cloud;
        let mut tour = Tour::new(
            Flow::with_engine_setup(
                Step::Welcome,
                platform.setup_status(installed),
                platform.caps.permissions,
                platform.caps.practice_dictation,
            ),
            false,
            cloud,
        );
        tour.replayed = true;
        tour
    }

    /// The tour when this run still needs it: a new install, or a run quit
    /// part-way (resuming at its step). Never after "Finish later".
    pub(super) fn start(
        settings: &AppSettings,
        platform: &PlatformState,
        picker: &super::picker::Picker,
    ) -> Option<Tour> {
        let directory = crate::app_paths::support_dir().ok();
        Self::start_at(directory.as_deref(), settings, platform, picker)
    }

    /// [`Tour::start`] reading the markers in `directory` (none: a profile
    /// whose data folder can't be found, which counts as new).
    pub(super) fn start_at(
        directory: Option<&std::path::Path>,
        settings: &AppSettings,
        platform: &PlatformState,
        picker: &super::picker::Picker,
    ) -> Option<Tour> {
        let completed = directory.is_some_and(crate::desktop::onboarding::completion_recorded_at);
        let step = match directory {
            Some(directory) => crate::desktop::onboarding::launch_step_at(
                directory,
                completed,
                crate::desktop::onboarding::unix_ms(),
            ),
            None => Some(Step::Welcome),
        }?;
        // The keyring is asked only for a run that opens the tour.
        let cloud = key_already_set(settings);
        let installed = picker.selected_installed(settings) || cloud;
        Some(Tour::new(
            Flow::with_engine_setup(
                step,
                platform.setup_status(installed),
                platform.caps.permissions,
                platform.caps.practice_dictation,
            ),
            completed,
            cloud,
        ))
    }

    /// The tour reopened at `step` from "Set up dictation".
    fn reopen(
        settings: &AppSettings,
        platform: &PlatformState,
        picker: &super::picker::Picker,
        step: Step,
    ) -> Tour {
        let cloud = key_already_set(settings);
        let installed = picker.selected_installed(settings) || cloud;
        Tour::new(
            Flow::with_engine_setup(
                step,
                platform.setup_status(installed),
                platform.caps.permissions,
                platform.caps.practice_dictation,
            ),
            true,
            cloud,
        )
    }
}

/// "Finish later" was chosen and setup is still unfinished: the app opens
/// with a "Set up dictation" row until microphone, accessibility and engine
/// are all ready.
pub(super) struct Deferred {
    /// A Groq key was set (and the engine isn't Local) when the choice was
    /// made or the app launched. The keyring is asked then, not every frame.
    cloud_key: bool,
}

impl Deferred {
    /// At launch: deferred setup from an earlier run, when no tour opens.
    pub(super) fn at_launch(ui: &Ui) -> Option<Deferred> {
        (ui.tour.is_none() && !ui.is_fixture() && crate::desktop::onboarding::deferred_recorded())
            .then(|| Deferred {
                cloud_key: key_already_set(&ui.settings),
            })
    }
}

/// What "Set up dictation" names: the first missing piece, while setup is
/// deferred. None once microphone, accessibility and the engine are ready.
pub(super) fn setup_gap(ui: &Ui) -> Option<SetupGap> {
    let deferred = ui.deferred.as_ref()?;
    if ui.tour.is_some() {
        return None;
    }
    let installed = ui.picker.selected_installed(&ui.settings);
    crate::desktop::onboarding::setup_gap(
        ui.platform.setup_status(installed),
        installed || deferred.cloud_key,
    )
}

/// "Set up dictation" (or the dictation shortcut): the tour reopens at the
/// step that fixes the first missing piece.
pub(super) fn reopen_setup(ui: &mut Ui) {
    let Some(gap) = setup_gap(ui) else {
        return;
    };
    ui.deferred = None;
    if !ui.is_fixture() {
        crate::desktop::onboarding::clear_deferred();
    }
    ui.tour = Some(Tour::reopen(
        &ui.settings,
        &ui.platform,
        &ui.picker,
        gap.step(),
    ));
    ui.first_run = None;
    super::onboarding_kit::live::open(ui);
}

/// Whether the listener's log shows a press of the dictation shortcut: a
/// dictation began, or failed to start for want of an engine.
fn pressed_shortcut(heard: &[Input]) -> bool {
    heard.iter().any(|input| {
        matches!(
            input,
            Input::Dictation(Observation::Started | Observation::Failed)
        )
    })
}

/// While setup is deferred: clears the row once it is complete, and reopens
/// the tour when the dictation shortcut is pressed. True when it changed.
fn poll_deferred(ui: &mut Ui) -> bool {
    if ui.deferred.is_none() || ui.tour.is_some() {
        return false;
    }
    if setup_gap(ui).is_none() {
        ui.deferred = None;
        if !ui.is_fixture() {
            crate::desktop::onboarding::clear_deferred();
        }
        return true;
    }
    // The listener logs the press; the practice feed is how the window hears
    // it. Anything it logs after the row appeared is the shortcut.
    let pressed = pressed_shortcut(&ui.host.tour_inputs(true));
    if pressed {
        reopen_setup(ui);
    }
    pressed
}

/// A cloud key was set when the tour opened or saved on its engine step.
pub(super) fn cloud_ready(ui: &Ui) -> bool {
    ui.tour.as_ref().is_some_and(|tour| {
        tour.cloud_ready
            || tour
                .flow
                .view()
                .engine
                .is_some_and(|engine| engine.key_saved)
    })
}

/// An engine is ready: the selected model is installed, or the cloud is set.
pub(super) fn engine_installed(ui: &Ui) -> bool {
    ui.picker.selected_installed(&ui.settings) || cloud_ready(ui)
}

pub(super) fn progress(ui: &Ui) -> (bool, ModelProgress) {
    let selection = super::onboarding_kit::engine_setup::selection(ui);
    let selected = crate::transcription::models::validate(&selection).ok();
    let local = selected.is_some_and(|model| ui.picker.installed(model, &selection.language));
    let installed = local || cloud_ready(ui);
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    let downloading = selected.and_then(|model| {
        (transcription.and_then(|t| t.preparing) == Some(model.id)).then(|| {
            model.download_bytes().map_or(0.0, |bytes| {
                (transcription.map_or(0, |t| t.downloaded_bytes) as f32 / bytes as f32)
                    .clamp(0.0, 1.0)
            })
        })
    });
    (
        installed,
        ModelProgress {
            installed: local || cloud_ready(ui),
            downloading,
            failed: downloading.is_none() && transcription.is_some_and(|t| t.error.is_some()),
        },
    )
}

/// Feeds the flow fresh permission and model state; true when the tour
/// changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    if ui.tour.is_none() {
        return poll_deferred(ui);
    }
    // Onboarding looks for models already on this computer once, and holds
    // its own download until the scan answers; with one found, the model
    // list (`found_choice`) starts the row the person picks.
    if ui.tour.is_some() {
        super::found::scan_for_setup(ui);
        // Only until setup: past it, the recommended model downloads.
        let held = (ui.found.scanning() || !ui.found.models.is_empty())
            && ui
                .tour
                .as_ref()
                .is_some_and(|tour| matches!(tour.flow.step(), Step::Welcome | Step::Setup));
        let choosing = super::found_choice::offered(
            ui,
            ui.tour
                .as_ref()
                .map_or(ItemStatus::Todo, |tour| tour.flow.view().model),
        );
        if choosing && ui.found.recommended.is_none() {
            ui.found.recommended = Some(ui.settings.transcription.clone());
        }
        if let Some(tour) = &mut ui.tour {
            tour.flow.hold_download(held);
        }
    }
    probe_hardware(ui);
    let (installed, model) = progress(ui);
    let status = ui.platform.setup_status(installed);
    let accessibility_stale = ui
        .platform
        .stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    let live = ui.live();
    let Some(tour) = &mut ui.tour else {
        return false;
    };
    let before = tour.flow.view();
    let step = before.step;
    let mut effect = tour.flow.on(Input::Status(status, model));
    tour.flow.set_accessibility_stale(accessibility_stale);
    if tour
        .advance_at
        .is_some_and(|at| at.elapsed() >= ADVANCE_BEAT)
    {
        tour.advance_at = None;
        if tour.flow.step() == Step::Setup {
            effect = tour.flow.on(Input::Continue);
        }
    }
    if status.ready() && !tour.completed {
        tour.completed = true;
        if live && let Err(error) = crate::desktop::onboarding::record_completion() {
            tracing::warn!(%error, "could not record onboarding completion");
        }
    }
    let practicing = tour.flow.step() == Step::TryIt;
    apply(ui, effect);
    let heard = ui.host.tour_inputs(practicing);
    let changed = !heard.is_empty();
    for heard in heard {
        input(ui, heard);
    }
    remember_step(ui, step);
    let after = ui.tour.as_ref().map(|tour| tour.flow.view());
    changed || after.as_ref() != Some(&before)
}

/// Starts the hardware probe with the tour and hands the flow the
/// recommendation when it answers. A scene or test sets `engine_setup.probed`
/// itself and never probes.
fn probe_hardware(ui: &mut Ui) {
    if ui.tour.is_none() {
        return;
    }
    if ui.engine_setup.probed.is_none() && !ui.is_fixture() {
        crate::hardware::start();
        ui.engine_setup.probed = crate::hardware::cached();
    }
    let Some(probed) = ui.engine_setup.probed.clone() else {
        return;
    };
    let waiting = ui
        .tour
        .as_ref()
        .and_then(|tour| tour.flow.view().engine)
        .is_some_and(|engine| engine.recommendation.is_none());
    if waiting {
        input(ui, Input::Recommend(probed.recommendation()));
    }
}

pub(super) fn input(ui: &mut Ui, input: Input) {
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let step = tour.flow.step();
    let effect = tour.flow.on(input);
    apply(ui, effect);
    remember_step(ui, step);
}

/// Saves a new step so a run quit part-way resumes there; the last page has
/// nothing to resume. The story sounds the success chime itself, on
/// WordInk is ready.
fn remember_step(ui: &Ui, before: Step) {
    if let Some(tour) = &ui.tour
        && tour.flow.step() != before
        && !ui.is_fixture()
    {
        match tour.flow.step() {
            Step::Done => crate::desktop::onboarding::clear_step(),
            step => crate::desktop::onboarding::record_step(step),
        }
    }
}

fn apply(ui: &mut Ui, effect: Effect) {
    match effect {
        Effect::DownloadModel => {
            // The model onboarding's choice picked, else the recommended one.
            let selection = ui
                .found
                .choice
                .take()
                .unwrap_or_else(|| super::onboarding_kit::engine_setup::selection(ui));
            if crate::transcription::models::validate(&selection).is_ok() {
                ui.send(Request::ChooseModel {
                    model: selection.model,
                    language: selection.language,
                });
            }
        }
        Effect::AdvanceSoon => {
            if let Some(tour) = &mut ui.tour {
                tour.advance_at = Some(Instant::now());
            }
        }
        Effect::RequestMicrophone => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::RequestMicrophone,
        )),
        Effect::ShowGuide(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ShowGuide(kind),
        )),
        Effect::ResetPermission(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ResetAndPrompt(kind),
        )),
        Effect::Relaunch => ui.ask(PlatformRequest::Relaunch),
        Effect::Finish => {
            ui.tour = None;
            ui.first_run = None;
        }
        Effect::Defer => {
            // The key was set before this point (a saved one counts), so
            // remember it: the keyring is not asked again every frame.
            let cloud_key = cloud_ready(ui);
            if !ui.is_fixture()
                && let Err(error) = crate::desktop::onboarding::record_deferred()
            {
                tracing::warn!(%error, "could not record that setup was deferred");
            }
            ui.deferred = Some(Deferred { cloud_key });
            ui.tour = None;
            ui.first_run = None;
        }
        Effect::None => {}
    }
}

/// A first run at `step`, with the selected model not yet installed.
pub(super) fn fixture(ui: &mut Ui, step: Step, downloading: bool) {
    ui.picker.installed = Some(Vec::new());
    let status = ui.platform.setup_status(false);
    let mut flow = Flow::new(
        Step::Welcome,
        status,
        ui.platform.caps.permissions,
        ui.platform.caps.practice_dictation,
    );
    flow.hold_download(!ui.found.models.is_empty());
    if step != Step::Welcome {
        flow.on(Input::Continue);
    }
    ui.tour = Some(Tour::new(flow, false, false));
    if downloading {
        input(ui, Input::Grant(SetupItem::Model));
        set_downloaded(ui, 62);
        poll(ui);
    }
}

/// A first run at the engine step on a computer that probed as `probed`
/// (scenes and tests): nothing installed, no key, permissions on.
pub(super) fn engine_fixture(ui: &mut Ui, probed: crate::hardware::Probed) {
    ui.engine_setup.probed = Some(probed);
    ui.picker.installed = Some(Vec::new());
    let status = ui.platform.setup_status(false);
    let flow = Flow::with_engine_setup(
        Step::Welcome,
        status,
        ui.platform.caps.permissions,
        ui.platform.caps.practice_dictation,
    );
    ui.tour = Some(Tour::new(flow, false, false));
    // Past the welcome and the demo.
    input(ui, Input::Continue);
    input(ui, Input::Skip);
    // One poll hands the flow the recommendation.
    poll(ui);
}

pub(super) fn set_downloaded(ui: &mut Ui, percent: u64) {
    if let Some(view) = &mut ui.view {
        let model = ui.settings.transcription.model;
        let bytes = crate::transcription::models::definition(model)
            .download_bytes()
            .unwrap_or(0);
        view.transcription.preparing = Some(model);
        view.transcription.downloaded_bytes = bytes * percent / 100;
    }
}

/// The macOS tour's states, walked as a person would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MacStep {
    Setup,
    SetupStale,
    SetupDenied,
    /// Every permission on, Accessibility waiting for a restart.
    SetupRestart,
    TryIt,
    /// The shortcut listener failed after setup.
    TryItReopen,
    Done,
}

/// Walks a fresh macOS first run to `state`. `ui.platform` must carry
/// macOS capabilities.
pub(super) fn mac_fixture(ui: &mut Ui, state: MacStep) {
    use PermissionState::{NeedsRequest, NeedsRestart, NeedsSettings, Ready};
    let fresh = crate::desktop::onboarding::SetupStatus {
        microphone: NeedsRequest,
        accessibility: NeedsSettings,
        transcription_model: false,
    };
    let installed = matches!(state, MacStep::TryIt | MacStep::TryItReopen | MacStep::Done);
    let permissions = match state {
        MacStep::Setup => fresh,
        MacStep::SetupStale => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            ..fresh
        },
        MacStep::SetupDenied => crate::desktop::onboarding::SetupStatus {
            microphone: NeedsSettings,
            ..fresh
        },
        MacStep::SetupRestart => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: NeedsRestart,
            ..fresh
        },
        _ => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: Ready,
            transcription_model: true,
        },
    };
    ui.platform.permissions = fresh;
    ui.picker.installed = Some(Vec::new());
    ui.tour = Some(Tour::new(
        Flow::new(Step::Welcome, fresh, true, true),
        false,
        false,
    ));
    // The tour starts the download as soon as it sees no model.
    poll(ui);
    input(ui, Input::Continue);
    if state == MacStep::SetupStale {
        for item in [SetupItem::Microphone, SetupItem::Accessibility] {
            input(ui, Input::Grant(item));
        }
        ui.platform
            .note_stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    }
    ui.platform.permissions = permissions;
    if installed {
        ui.picker.installed = Some(vec![ui.settings.transcription.model]);
        if let Some(view) = &mut ui.view {
            view.transcription.preparing = None;
        }
    } else {
        set_downloaded(ui, 31);
    }
    poll(ui);
    if installed {
        input(ui, Input::Continue);
    }
    if let Some(tour) = &mut ui.tour {
        tour.advance_at = None;
    }
    if state == MacStep::TryItReopen {
        input(
            ui,
            Input::ListenerFailed {
                relaunch_allowed: false,
            },
        );
    }
    if state == MacStep::Done {
        input(ui, Input::Skip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logged_dictation_start_or_failure_is_the_shortcut_press() {
        assert!(pressed_shortcut(&[Input::Dictation(Observation::Started)]));
        assert!(pressed_shortcut(&[Input::Dictation(Observation::Failed)]));
        assert!(!pressed_shortcut(&[Input::Dictation(
            Observation::Cancelled
        )]));
        assert!(!pressed_shortcut(&[Input::Continue]));
        assert!(!pressed_shortcut(&[]));
    }

    #[test]
    fn mac_stale_fixture_marks_accessibility_stale_and_dispatches_reset() {
        let mut ui = crate::rune_ui::mac_fixture();
        mac_fixture(&mut ui, MacStep::SetupStale);
        let rows = ui.tour.as_ref().unwrap().flow.view().rows;
        assert!(
            rows.iter()
                .any(|row| row.item == SetupItem::Accessibility && row.status == ItemStatus::Stale)
        );
        input(&mut ui, Input::Grant(SetupItem::Accessibility));
        assert!(
            !ui.platform
                .stale(crate::desktop::onboarding::PermissionKind::Accessibility)
        );
        match &ui.host {
            crate::rune_ui::platform::PlatformHost::Fixture(sent) => {
                assert_eq!(
                    sent.last(),
                    Some(&PlatformRequest::Permission(
                        crate::desktop::onboarding::PermissionAction::ResetAndPrompt(
                            crate::desktop::onboarding::PermissionKind::Accessibility
                        )
                    ))
                );
            }
            _ => panic!("expected fixture host"),
        }
    }
}
