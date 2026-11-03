//! General › Advanced › Debug logs: the switch and level for the opt-in
//! debug log (`crate::debug_log`), and the buttons that open, copy and clear
//! it. What it records never includes keys, audio or (unless opted in)
//! transcripts, and "Copy recent debug log" scrubs once more.

use rune::{El, Tx, row, text};

use super::Ui;
use super::controls::{ButtonKind, CardRow, DESCRIPTION, Paint, button, icon_row, icon_toggle_row};
use super::general::pick_row;
use super::icons::{FOLDER, SPARKLES, TERMINAL};
use crate::debug_log::{self, DebugLevel};
use crate::desktop::i18n::{tr, tr_fill};

/// What the last button press did, shown under the buttons.
#[derive(Default)]
pub(super) struct DebugLogsRow {
    note: Option<Note>,
}

struct Note {
    message: String,
    failed: bool,
}

impl DebugLogsRow {
    fn done(&mut self, message: String) {
        self.note = Some(Note {
            message,
            failed: false,
        });
    }

    fn failed(&mut self, message: String) {
        self.note = Some(Note {
            message,
            failed: true,
        });
    }
}

/// The path shown to the person; unit tests show a fixed one, since the real
/// folder differs per machine.
fn shown_path() -> String {
    if cfg!(test) {
        return "logs/debug.log".into();
    }
    debug_log::path().map_or_else(
        || tr("The logs folder is unavailable").to_owned(),
        |path| path.display().to_string(),
    )
}

pub(super) fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = Vec::new();
    rows.push(
        icon_toggle_row(
            p,
            "developer-mode",
            &TERMINAL,
            tr("Developer mode"),
            tr("Opens the live log view and saves debug logs to a file. Events are always kept in memory. Never keys, audio or what you said."),
            settings.developer_mode,
            |ui: &mut Ui| {
                let on = !ui.settings.developer_mode;
                ui.update(|settings| settings.developer_mode = on);
                if on {
                    ui.log_view.open();
                } else {
                    ui.log_view.close();
                }
            },
        )
        .into(),
    );
    rows.push(pick_row(
        ui,
        p,
        "debug-logging-level",
        &SPARKLES,
        tr("Detail"),
        DebugLevel::ALL
            .iter()
            .map(|level| tr(level.label()))
            .collect(),
        DebugLevel::ALL
            .iter()
            .position(|level| *level == settings.debug_logging_level)
            .unwrap_or(0),
        |ui, index| {
            let level = DebugLevel::ALL[index];
            if ui.settings.debug_logging_level != level {
                ui.update(|settings| settings.debug_logging_level = level);
            }
        },
        layer,
    ));
    rows.push(
        icon_row(
            p,
            "debug-logs-viewer",
            &TERMINAL,
            tr("Live log view"),
            Some(tr(
                "Everything captured so far, then new events as they happen",
            )),
            button(
                p,
                "debug-logs-view",
                tr("Open log viewer"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _: &mut Tx| ui.log_view.open(),
            ),
        )
        .into(),
    );
    let buttons = row()
        .key("debug-logs-actions")
        .gap(8.0)
        .center_y()
        .child(button(
            p,
            "debug-logs-open",
            tr("Open logs folder"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| open_folder(ui),
        ))
        .child(button(
            p,
            "debug-logs-copy",
            tr("Copy recent debug log"),
            ButtonKind::Secondary,
            |ui: &mut Ui, tx: &mut Tx| copy_recent(ui, tx),
        ))
        .child(button(
            p,
            "debug-logs-clear",
            tr("Clear debug logs"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| clear(ui),
        ));
    rows.push(
        icon_row(
            p,
            "debug-logs-files",
            &FOLDER,
            tr("Debug log"),
            Some(&shown_path()),
            buttons,
        )
        .into(),
    );
    if let Some(note) = &ui.debug_logs.note {
        rows.push(
            icon_row(
                p,
                "debug-logs-note",
                &FOLDER,
                tr("Debug logs"),
                None,
                text(note.message.clone())
                    .key("debug-logs-note-text")
                    .font(DESCRIPTION)
                    .color(if note.failed { p.danger() } else { p.muted() })
                    .wrap(),
            )
            .into(),
        );
    }
    rows
}

fn open_folder(ui: &mut Ui) {
    if cfg!(test) {
        return;
    }
    match crate::app_paths::logs_dir() {
        Ok(dir) => {
            let _ = std::fs::create_dir_all(&dir);
            crate::meeting::reveal(&dir);
            ui.debug_logs.note = None;
        }
        Err(error) => ui.debug_logs.failed(format!("{error:#}")),
    }
}

fn copy_recent(ui: &mut Ui, tx: &mut Tx) {
    let result = if cfg!(test) {
        Err(tr("There is no debug log yet").to_owned())
    } else {
        debug_log::recent(debug_log::COPY_LINES, &debug_log::Filter::default())
            .map_err(|error| tr_fill("Could not copy: {}", &error))
    };
    match result {
        Ok(text) => {
            let lines = text.lines().count().to_string();
            tx.copy(text);
            ui.debug_logs.done(tr_fill(
                "Copied the last {} lines, scrubbed of paths and keys",
                &lines,
            ));
        }
        Err(message) => ui.debug_logs.failed(message),
    }
}

fn clear(ui: &mut Ui) {
    if cfg!(test) {
        return;
    }
    ui.log_view.clear_events();
    match debug_log::clear() {
        Ok(0) => ui
            .debug_logs
            .done(tr("There was no debug log to clear").to_owned()),
        Ok(count) => ui
            .debug_logs
            .done(tr_fill("Cleared {} debug log files", &count.to_string())),
        Err(error) => ui.debug_logs.failed(error),
    }
}
