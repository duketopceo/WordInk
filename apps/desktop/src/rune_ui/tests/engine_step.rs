//! The first run's engine step (F1) and the settings panes R27 adds, driven
//! through the window.

use super::*;
use crate::desktop::onboarding::{EngineKind, Step};
use crate::transcription::cloud::EngineChoice;

const SECRET: &str = "gsk_window_secret_0123456789";

fn step_of(rt: &Runtime<Ui>) -> Step {
    rt.state.tour.as_ref().unwrap().flow.step()
}

fn present(rt: &Runtime<Ui>, key: &str) -> bool {
    rt.find(key).is_some()
}

#[test]
fn the_engine_step_shows_the_recommendation_and_one_alternative() {
    // No GPU: the cloud is recommended, local is the alternative.
    let rt = runtime("onboarding-engine", true, PANE_SIZE);
    assert_eq!(step_of(&rt), Step::Engine);
    let view = rt.state.tour.as_ref().unwrap().flow.view().engine.unwrap();
    assert_eq!(view.chosen, None);
    assert!(present(&rt, "option-cloud") && present(&rt, "option-local"));
    assert!(!present(&rt, "continue"), "nothing is chosen yet");
    let cloud = element(&rt, "option-cloud").rect;
    let local = element(&rt, "option-local").rect;
    assert!(cloud[1] < local[1], "the recommended choice comes first");
    assert!(
        texts(&rt).iter().any(|t| t == "Recommended"),
        "one choice is tagged"
    );
    // A GPU: local first.
    let rt = runtime("onboarding-engine-local", true, PANE_SIZE);
    let cloud = element(&rt, "option-cloud").rect;
    let local = element(&rt, "option-local").rect;
    assert!(local[1] < cloud[1]);
}

#[test]
fn the_cloud_path_keeps_the_key_masked_and_clears_it_on_save() {
    let mut rt = runtime("onboarding-engine", true, PANE_SIZE);
    tap(&mut rt, "option-cloud");
    assert!(present(&rt, "key-field"));
    assert!(!present(&rt, "continue"), "Continue waits for the key");

    rt.state.engine_setup.key = TextEdit::new(SECRET.to_string()).secure();
    rt.invalidate();
    settle(&mut rt);
    assert!(
        !texts(&rt).iter().any(|t| t.contains(SECRET)),
        "the field is masked while typing"
    );
    tap(&mut rt, "save-key");

    assert!(
        rt.state.engine_setup.key.text.is_empty(),
        "the field is cleared"
    );
    assert_eq!(rt.state.settings.engine, EngineChoice::Groq);
    let view = rt.state.tour.as_ref().unwrap().flow.view().engine.unwrap();
    assert!(view.key_saved && view.can_continue);
    assert!(present(&rt, "continue"));
    assert!(!present(&rt, "key-field"), "the field is gone once saved");
    assert!(texts(&rt).iter().all(|t| !t.contains(SECRET)));
    assert!(
        sent(&rt)
            .iter()
            .all(|request| !matches!(request, Request::ChooseModel { .. })),
        "the cloud path downloads nothing"
    );

    // Hotkey, then the test dictation.
    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::Hotkey);
    assert!(present(&rt, "hotkey"));
    tap(&mut rt, "continue");
    assert_eq!(step_of(&rt), Step::TryIt);
}

#[test]
fn an_empty_key_is_refused_without_leaving_the_step() {
    let mut rt = runtime("onboarding-engine-cloud", true, PANE_SIZE);
    tap(&mut rt, "save-key");
    assert_eq!(step_of(&rt), Step::Engine);
    assert!(present(&rt, "key-error"), "the refusal is said");
    assert!(
        !rt.state
            .tour
            .as_ref()
            .unwrap()
            .flow
            .view()
            .engine
            .unwrap()
            .key_saved
    );
}

#[test]
fn the_local_path_starts_the_parakeet_v3_download() {
    let mut rt = runtime("onboarding-engine", true, PANE_SIZE);
    tap(&mut rt, "option-local");
    let asked = sent(&rt)
        .iter()
        .filter(|request| {
            matches!(
                request,
                Request::ChooseModel { model, .. } if *model == TranscriptionModelId::ParakeetV3
            )
        })
        .count();
    assert_eq!(asked, 1, "one download of Parakeet v3");
    assert_eq!(rt.state.settings.engine, EngineChoice::Local);
    let view = rt.state.tour.as_ref().unwrap().flow.view().engine.unwrap();
    assert_eq!(view.chosen, Some(EngineKind::Local));
    assert!(present(&rt, "download-progress"), "its progress shows");
    assert!(present(&rt, "continue"), "the tour goes on while it runs");
}

#[test]
fn the_sidebar_lists_the_new_panes_and_each_opens() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    for (key, pane) in [
        ("nav-replacements", Pane::Replacements),
        ("nav-sounds", Pane::Sounds),
        ("nav-privacy", Pane::Privacy),
    ] {
        tap(&mut rt, key);
        assert_eq!(rt.state.pane, pane, "{key}");
    }
}

#[test]
fn replacements_edit_the_global_mode() {
    let mut rt = runtime("replacements", true, PANE_SIZE);
    assert!(present(&rt, "correction0") && present(&rt, "correction2"));
    tap(&mut rt, "add-correction");
    assert!(present(&rt, "correction3"));
    rt.paste("gonna");
    settle(&mut rt);
    flush(&mut rt);
    let saved = &rt
        .state
        .settings
        .dictation_processing
        .default_mode
        .replacements;
    assert_eq!(saved.len(), 4);
    assert_eq!(saved[3].matched_phrase, "gonna");
    tap(&mut rt, "correction0/remove");
    flush(&mut rt);
    let saved = &rt
        .state
        .settings
        .dictation_processing
        .default_mode
        .replacements;
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[0].matched_phrase, "dash dash");
}

#[test]
fn sounds_holds_the_volume_and_the_set() {
    let rt = runtime("sounds", true, PANE_SIZE);
    assert!(present(&rt, "sound-volume") && present(&rt, "sound-set-control"));
}

#[test]
fn privacy_says_what_leaves_and_how_long_things_are_kept() {
    let mut rt = runtime("privacy", true, PANE_SIZE);
    for key in [
        "privacy-audio",
        "privacy-downloads",
        "privacy-telemetry",
        "privacy-keys",
        "privacy-history",
        "privacy-clips",
    ] {
        assert!(present(&rt, key), "{key}");
    }
    let shown = texts(&rt).join("\n");
    assert!(shown.contains("huggingface.co"));
    assert!(shown.contains("None. WordInk sends no analytics"));
    assert!(shown.contains("Kept for 7 days"), "{shown}");
    rt.state.settings.history_retention = crate::history::HistoryRetention::Off;
    rt.invalidate();
    settle(&mut rt);
    assert!(texts(&rt).join("\n").contains("Nothing is kept"));
}

#[test]
fn privacy_offers_copy_diagnostics_and_keeps_the_no_analytics_copy() {
    // Tall enough to show the Diagnostics section, which is last.
    let mut rt = runtime("privacy", true, (PANE_SIZE.0, 1400.0));
    assert!(present(&rt, "privacy-diagnostics"));
    assert!(!present(&rt, "privacy-diagnostics-text"), "closed at first");
    let shown = texts(&rt).join("\n");
    assert!(shown.contains("None. WordInk sends no analytics, crash reports or usage data."));
    assert!(shown.contains("Copy diagnostics"));

    tap(&mut rt, "privacy-diagnostics-toggle");
    assert!(present(&rt, "privacy-diagnostics-text"));
    let shown = texts(&rt).join("\n");
    assert!(shown.contains("Version: "), "{shown}");
    assert!(shown.contains("Engine: "), "{shown}");
    assert!(shown.contains("Recent errors: none"), "{shown}");
    assert!(shown.contains("None. WordInk sends no analytics"));
    assert!(rt.copy_request.is_none(), "opening the row copies nothing");
    assert!(!present(&rt, "privacy-diagnostics-copied"));

    tap(&mut rt, "privacy-diagnostics-copy");
    let copied = rt.copy_request.take().expect("Copy writes the clipboard");
    assert_eq!(copied.content().text(), rt.state.diagnostics.text.as_str());
    assert!(present(&rt, "privacy-diagnostics-copied"));
    assert!(texts(&rt).iter().any(|t| t == "Copied"));

    tap(&mut rt, "privacy-diagnostics-toggle");
    assert!(!present(&rt, "privacy-diagnostics-text"));
}

#[test]
fn a_failed_diagnostics_copy_shows_the_error_and_keeps_the_text() {
    let mut rt = runtime("privacy", true, (PANE_SIZE.0, 1400.0));
    tap(&mut rt, "privacy-diagnostics-toggle");
    rt.state.diagnostics.error = Some("no clipboard".into());
    rt.invalidate();
    settle(&mut rt);
    let shown = texts(&rt).join("\n");
    assert!(shown.contains("Could not copy: no clipboard."), "{shown}");
    assert!(present(&rt, "privacy-diagnostics-text"));
    assert!(shown.contains("Version: "));
}

#[test]
fn leaving_privacy_closes_the_diagnostics_row() {
    let mut rt = runtime("privacy", true, (PANE_SIZE.0, 1400.0));
    tap(&mut rt, "privacy-diagnostics-toggle");
    assert!(present(&rt, "privacy-diagnostics-text"));
    tap(&mut rt, "nav-general");
    tap(&mut rt, "nav-privacy");
    assert!(
        !present(&rt, "privacy-diagnostics-text"),
        "the summary is rebuilt on the next open"
    );
}
