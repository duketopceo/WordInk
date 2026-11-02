//! The Privacy pane: what leaves this computer, where keys live, and how
//! long dictations and clips are kept. Read-only; the controls it describes
//! live in General (engine), Models (downloads) and History (retention).

use rune::{El, el};

use super::Ui;
use super::controls::{CardRow, Paint, icon_row, note, page, section};
use super::icons::{FOLDER, GLOBE, LOCK, MIC, SPARKLES};
use crate::app_settings::AppSettings;
use crate::desktop::i18n::{tr, tr_fill};

/// How long history keeps dictations, in words (the same labels History
/// offers).
pub(super) fn retention_copy(settings: &AppSettings) -> String {
    let retention = settings.history_retention;
    if retention.is_off() {
        tr("Nothing is kept").to_string()
    } else {
        tr_fill("Kept for {}", tr(retention.label()))
    }
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let cloud = ui.settings.engine != crate::transcription::cloud::EngineChoice::Local;
    let leaves = vec![
        value_row(
            p,
            "privacy-audio",
            &MIC,
            tr("Your voice"),
            tr(if cloud {
                "Stays on this computer while a local model transcribes. A cloud engine (Groq), when you choose one or add its key, receives each recording to transcribe it."
            } else {
                "Stays on this computer. Only a cloud engine, if you choose one, receives recordings."
            }),
        ),
        value_row(
            p,
            "privacy-downloads",
            &GLOBE,
            tr("Model downloads"),
            tr("Local models download from huggingface.co. Nothing else is sent with the request."),
        ),
        value_row(
            p,
            "privacy-updates",
            &SPARKLES,
            tr("Update checks"),
            tr("Ask for the latest release, and send no usage data."),
        ),
    ];
    let kept = vec![
        value_row(
            p,
            "privacy-telemetry",
            &SPARKLES,
            tr("Telemetry"),
            tr("None. WordInk sends no analytics, crash reports or usage data."),
        ),
        value_row(
            p,
            "privacy-keys",
            &LOCK,
            tr("Where your API keys are stored"),
            tr(
                "In the OS keyring (or, where there is none, a private file beside your settings), or read from GROQ_API_KEY. Never in the settings file or a log.",
            ),
        ),
        value_row(
            p,
            "privacy-history",
            &FOLDER,
            tr("History"),
            &retention_copy(&ui.settings),
        ),
        value_row(
            p,
            "privacy-clips",
            &FOLDER,
            tr("Audio clips"),
            tr(
                "Only a cloud dictation that failed keeps its clip, so History can retry it, and only for the history time above. Successful dictations keep no audio.",
            ),
        ),
    ];
    let content = page("content")
        .child(section(
            p,
            "leaves",
            tr("What leaves this computer"),
            leaves,
        ))
        .child(section(p, "kept", tr("What stays"), kept))
        .child(note(
            "privacy-where",
            tr("Change the engine in General, retention in History."),
            p.muted(),
        ));
    super::scroll_body("scroll", content)
}

fn value_row(p: Paint, key: &str, glyph: &'static rune::Icon, title: &str, copy: &str) -> CardRow {
    icon_row(p, key, glyph, title, Some(copy), el().key("none")).into()
}
