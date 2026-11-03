//! The shorter first run (U7): welcome, one demo, permissions, engine,
//! shortcut, one test dictation, done; "Finish later" and the "Set up
//! dictation" row; and where a relaunch opens. Driven through the window.

use super::*;
use crate::desktop::onboarding::{
    EngineKind, Input, Observation, PermissionState, Practice, SetupStatus, Step,
};

fn step_of(rt: &Runtime<Ui>) -> Step {
    rt.state.tour.as_ref().unwrap().flow.step()
}

fn present(rt: &Runtime<Ui>, key: &str) -> bool {
    rt.find(key).is_some()
}

fn refresh(rt: &mut Runtime<Ui>) {
    rt.invalidate();
    settle(rt);
}

const GRANTED: SetupStatus = SetupStatus {
    microphone: PermissionState::Ready,
    accessibility: PermissionState::Ready,
    transcription_model: false,
};

/// A first run on Linux with nothing installed and no key.
fn fresh_first_run() -> Ui {
    let mut ui = portal_fixture();
    ui.platform.caps = platform::Capabilities::LINUX;
    ui.platform.permissions = GRANTED;
    ui.picker.installed = Some(Vec::new());
    ui.tour = Some(onboarding::Tour::replay(
        &ui.settings,
        &ui.platform,
        &ui.picker,
    ));
    onboarding_kit::live::open(&mut ui);
    ui
}

fn onboarding_picker() -> picker::Picker {
    let mut picker = picker::Picker::default();
    picker.installed = Some(Vec::new());
    picker
}

/// A temporary data folder for the launch markers; never the real one.
fn data_folder(name: &str) -> std::path::PathBuf {
    let folder =
        std::env::temp_dir().join(format!("wordink-first-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn a_fresh_profile_walks_every_page_with_no_practice_game() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    assert_eq!(step_of(&rt), Step::Welcome);
    assert!(texts(&rt).contains(&"Welcome to WordInk".to_owned()));
    assert!(!present(&rt, "skip-demo"), "Skip belongs to the demo");

    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::Demo);
    assert!(present(&rt, "skip-demo"), "the demo can be skipped");
    assert!(
        sent(&rt).is_empty(),
        "nothing downloads before the engine step"
    );

    // Skip: permissions are on here, so on to the engine choice.
    tap(&mut rt, "skip-demo");
    assert_eq!(step_of(&rt), Step::Engine);
    assert!(present(&rt, "option-cloud") && present(&rt, "finish-later"));
    rt.update(|ui, _| {
        onboarding::input(ui, Input::ChooseEngine(EngineKind::Cloud));
        onboarding::input(ui, Input::KeyStored);
    });
    refresh(&mut rt);
    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::Hotkey);
    assert!(present(&rt, "hotkey-page"));
    assert!(!present(&rt, "finish-later"), "only Setup and Engine defer");
    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::TryIt);

    // The test dictation is the plain field, not the practice game.
    rt.update(|ui, _| onboarding_kit::live::fixture_resume(ui, Page::Try));
    settle(&mut rt);
    assert!(present(&rt, "try-live-editor"));
    assert!(
        !texts(&rt).contains(&"Clear your messages".to_owned()),
        "no practice game"
    );
    assert!(present(&rt, "skip-try"));
    rt.update(|ui, tx| {
        let s = ui.first_run.as_mut().unwrap();
        s.practice.text = "Testing live dictation in WordInk.".into();
        s.on_practice_change(tx);
    });
    refresh(&mut rt);
    tap(&mut rt, "continue");
    refresh(&mut rt);
    assert_eq!(step_of(&rt), Step::Done);
    assert!(texts(&rt).contains(&"WordInk is ready".to_owned()));
    tap(&mut rt, "start-using-hex");
    assert!(rt.state.tour.is_none() && rt.state.first_run.is_none());
    assert!(rt.state.deferred.is_none());
}

#[test]
fn skip_on_the_test_dictation_leaves_it() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    tap(&mut rt, "continue");
    tap(&mut rt, "skip-demo");
    rt.update(|ui, _| {
        onboarding::input(ui, Input::ChooseEngine(EngineKind::Cloud));
        onboarding::input(ui, Input::KeyStored);
    });
    refresh(&mut rt);
    tap(&mut rt, "continue");
    tap(&mut rt, "continue");
    rt.update(|ui, _| onboarding_kit::live::fixture_resume(ui, Page::Try));
    settle(&mut rt);
    tap(&mut rt, "skip-try");
    refresh(&mut rt);
    assert_eq!(step_of(&rt), Step::Done);
    assert!(texts(&rt).contains(&"WordInk is ready".to_owned()));
}

#[test]
fn a_ready_engine_skips_the_engine_step_and_still_shows_the_hotkey() {
    let mut ui = fresh_first_run();
    ui.picker.installed = Some(vec![ui.settings.transcription.model]);
    ui.first_run = None;
    ui.tour = Some(onboarding::Tour::replay(
        &ui.settings,
        &ui.platform,
        &ui.picker,
    ));
    onboarding_kit::live::open(&mut ui);
    let mut rt = runtime_for(ui, true, PANE_SIZE);
    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::Demo);
    tap(&mut rt, "skip-demo");
    assert_eq!(step_of(&rt), Step::Hotkey);
    assert!(present(&rt, "hotkey-page") && !present(&rt, "option-cloud"));
}

#[test]
fn the_demo_finishing_on_its_own_moves_the_tour_to_setup() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::Demo);
    // The demo's own pages end on the permissions page.
    rt.update(|ui, _| {
        ui.first_run.as_mut().unwrap().jump_to_permissions();
    });
    refresh(&mut rt);
    assert_eq!(step_of(&rt), Step::Engine, "granted permissions pass Setup");
}

#[test]
fn finish_later_on_the_engine_step_offline_lands_on_set_up_dictation() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    tap(&mut rt, "continue");
    tap(&mut rt, "skip-demo");
    assert_eq!(step_of(&rt), Step::Engine);
    // No key and no model: the only way on is Finish later.
    tap(&mut rt, "finish-later");
    assert!(rt.state.tour.is_none(), "the tour closes");
    assert!(rt.state.first_run.is_none());
    assert_eq!(rt.state.pane, Pane::General, "the main window opens");
    assert!(texts(&rt).contains(&"Set up dictation".to_owned()));
    assert!(texts(&rt).contains(&"Choose how WordInk transcribes".to_owned()));
    assert!(present(&rt, "setup-dictation"));
    let setup = element(&rt, "setup-dictation").rect;
    let basics = element(&rt, "basics").rect;
    assert!(setup[1] < basics[1], "the row is at the top of General");

    // Pressing it reopens the tour at the engine step.
    tap(&mut rt, "setup-dictation-open");
    assert_eq!(step_of(&rt), Step::Engine);
    assert!(present(&rt, "option-cloud"));
    assert!(rt.state.deferred.is_none());
}

#[test]
fn the_row_clears_once_setup_and_the_engine_are_complete() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    tap(&mut rt, "continue");
    tap(&mut rt, "skip-demo");
    tap(&mut rt, "finish-later");
    assert!(present(&rt, "setup-dictation"));
    rt.state.picker.installed = Some(vec![rt.state.settings.transcription.model]);
    refresh(&mut rt);
    assert!(!present(&rt, "setup-dictation"));
    assert!(rt.state.deferred.is_none());
}

#[test]
fn with_the_microphone_denied_the_row_names_it_and_reopens_setup() {
    let mut ui = portal_fixture();
    ui.platform = platform::PlatformState::preview(platform::Capabilities::MACOS);
    ui.host = platform::PlatformHost::Fixture(Vec::new());
    ui.platform.permissions = SetupStatus {
        microphone: PermissionState::NeedsSettings,
        accessibility: PermissionState::NeedsSettings,
        transcription_model: false,
    };
    ui.picker.installed = Some(Vec::new());
    ui.tour = Some(onboarding::Tour::replay(
        &ui.settings,
        &ui.platform,
        &ui.picker,
    ));
    onboarding_kit::live::open(&mut ui);
    let mut rt = runtime_for(ui, true, PANE_SIZE);
    tap(&mut rt, "continue");
    tap(&mut rt, "skip-demo");
    assert_eq!(step_of(&rt), Step::Setup);
    assert!(present(&rt, "finish-later"), "Setup offers Finish later");
    tap(&mut rt, "finish-later");
    assert!(rt.state.tour.is_none());
    let shown = texts(&rt);
    assert!(shown.contains(&"Set up dictation".to_owned()), "{shown:?}");
    assert!(
        shown.contains(&"Microphone access is off.".to_owned()),
        "{shown:?}"
    );
    tap(&mut rt, "setup-dictation-open");
    assert_eq!(step_of(&rt), Step::Setup);
    assert!(present(&rt, "mic-row"), "Setup shows the microphone");
}

#[test]
fn the_dictation_shortcut_reopens_setup_while_it_is_deferred() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    tap(&mut rt, "continue");
    tap(&mut rt, "skip-demo");
    tap(&mut rt, "finish-later");
    assert!(rt.state.tour.is_none());
    // The listener logs the press; `pressed_shortcut` reads it (tested
    // beside it) and the window reopens setup.
    rt.update(|ui, _| {
        onboarding::reopen_setup(ui);
    });
    refresh(&mut rt);
    assert_eq!(step_of(&rt), Step::Engine);
}

#[test]
fn a_relaunch_opens_where_the_last_run_stopped() {
    let settings = AppSettings::default();
    let mut ui = portal_fixture();
    ui.platform.caps = platform::Capabilities::LINUX;
    ui.platform.permissions = GRANTED;
    let platform = &ui.platform;
    let mut picker = onboarding_picker();
    picker.installed = Some(Vec::new());
    let picker = picker;

    // A new profile: the tour from Welcome.
    let folder = data_folder("launch-new");
    let tour = onboarding::Tour::start_at(Some(&folder), &settings, platform, &picker).unwrap();
    assert_eq!(tour.flow.step(), Step::Welcome);

    // Quit on Hotkey (setup was ready, so completion was recorded): Hotkey.
    crate::desktop::onboarding::record_completion_at(&folder).unwrap();
    crate::desktop::onboarding::record_step_at(&folder, Step::Hotkey).unwrap();
    let tour = onboarding::Tour::start_at(Some(&folder), &settings, platform, &picker).unwrap();
    assert_eq!(tour.flow.step(), Step::Hotkey);

    // Finish later: the app, not the tour.
    crate::desktop::onboarding::record_deferred_at(&folder).unwrap();
    assert!(onboarding::Tour::start_at(Some(&folder), &settings, platform, &picker).is_none());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_denied_microphone_resumes_at_setup_not_past_it() {
    let settings = AppSettings::default();
    let mut ui = portal_fixture();
    ui.platform = platform::PlatformState::preview(platform::Capabilities::MACOS);
    ui.platform.permissions = SetupStatus {
        microphone: PermissionState::NeedsSettings,
        accessibility: PermissionState::Ready,
        transcription_model: false,
    };
    let picker = onboarding_picker();
    let folder = data_folder("launch-denied");
    crate::desktop::onboarding::record_step_at(&folder, Step::Hotkey).unwrap();
    let tour = onboarding::Tour::start_at(Some(&folder), &settings, &ui.platform, &picker).unwrap();
    assert_eq!(tour.flow.step(), Step::Setup);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_test_dictation_tells_no_speech_from_an_engine_error() {
    let mut rt = runtime_for(fresh_first_run(), true, PANE_SIZE);
    tap(&mut rt, "continue");
    tap(&mut rt, "skip-demo");
    rt.update(|ui, _| {
        onboarding::input(ui, Input::ChooseEngine(EngineKind::Cloud));
        onboarding::input(ui, Input::KeyStored);
    });
    refresh(&mut rt);
    tap(&mut rt, "continue");
    tap(&mut rt, "continue");
    rt.update(|ui, _| onboarding_kit::live::fixture_resume(ui, Page::Try));
    settle(&mut rt);
    let heard = |rt: &mut Runtime<Ui>, observations: &[Observation]| {
        for observation in observations {
            onboarding::input(&mut rt.state, Input::Dictation(*observation));
        }
        refresh(rt);
        texts(rt)
    };
    let silent = heard(
        &mut rt,
        &[
            Observation::Started,
            Observation::Transcribing,
            Observation::Discarded,
        ],
    );
    assert!(
        silent.iter().any(|t| t.contains("didn't hear any words")),
        "{silent:?}"
    );
    let failed = heard(&mut rt, &[Observation::Started, Observation::Failed]);
    assert!(
        failed.iter().any(|t| t == "That didn't work. Try again."),
        "{failed:?}"
    );
    assert_eq!(
        rt.state.tour.as_ref().unwrap().flow.view().practice,
        Practice::Failed
    );
}

#[test]
fn no_first_run_scene_says_hex() {
    for scene in super::onboarding_check::SCENES {
        let rt = runtime(scene, true, PANE_SIZE);
        let shown = texts(&rt);
        // The sidebar under the first run carries the MIT attribution line.
        assert!(
            shown
                .iter()
                .filter(|text| *text != "Based on Hex by Anomaly (MIT)")
                .all(|text| !text.contains("Hex")),
            "{scene}: {shown:?}"
        );
    }
}
