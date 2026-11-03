//! The live debug log view: opened by turning Developer mode on (or its
//! "Open log viewer" button), pre-filled with every event captured so far
//! (`crate::debug_log` keeps them in memory all the time) and then streaming
//! new ones. Pause holds the stream (new events wait, none are lost), the
//! level and subsystem filters and search narrow it, and Copy visible, Clear
//! and Save to file act on what is shown.

use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

use rune::{El, ScrollSpec, Tx, col, row, text};
use rune_kit::TextInput;

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, METRICS, Paint, button};
use super::general::pick_select;
use crate::debug_log::{self, DebugLevel, Event, Filter, SUBSYSTEMS};
use crate::desktop::i18n::{tr, tr_fill};

/// Events the view holds; older ones scroll out (the ring keeps its own).
const KEPT: usize = 5_000;
/// Lines drawn: the newest matching ones, so a full buffer stays smooth.
const SHOWN: usize = 400;

pub(super) struct LogView {
    pub(super) open: bool,
    paused: bool,
    level: DebugLevel,
    subsystems: Vec<&'static str>,
    pub(super) search: rune::TextEdit,
    events: VecDeque<Event>,
    receiver: Option<Receiver<Event>>,
    note: Option<(String, bool)>,
}

impl Default for LogView {
    fn default() -> Self {
        Self {
            open: false,
            paused: false,
            level: DebugLevel::Trace,
            subsystems: Vec::new(),
            search: rune::TextEdit::new(String::new()),
            events: VecDeque::new(),
            receiver: None,
            note: None,
        }
    }
}

impl LogView {
    /// Opens the view on everything buffered, then the live stream.
    pub(super) fn open(&mut self) {
        let (history, receiver) = debug_log::subscribe();
        self.events = history.into_iter().collect();
        self.trim();
        self.receiver = Some(receiver);
        self.open = true;
        self.paused = false;
        self.note = None;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.receiver = None;
        self.events.clear();
    }

    /// Adds the events that arrived; a paused view leaves them queued.
    /// True when the view changed.
    pub(super) fn poll(&mut self) -> bool {
        if !self.open || self.paused {
            return false;
        }
        let Some(receiver) = &self.receiver else {
            return false;
        };
        let before = self.events.len();
        let arrived: Vec<Event> = receiver.try_iter().collect();
        let changed = !arrived.is_empty();
        self.events.extend(arrived);
        self.trim();
        changed || self.events.len() != before
    }

    pub(super) fn clear_events(&mut self) {
        self.events.clear();
    }

    fn trim(&mut self) {
        while self.events.len() > KEPT {
            self.events.pop_front();
        }
    }

    fn filter(&self) -> Filter {
        Filter {
            level: self.level,
            subsystems: self
                .subsystems
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            search: self.search.text.trim().to_owned(),
        }
    }

    /// The newest matching events, oldest first.
    fn visible(&self) -> Vec<&Event> {
        let filter = self.filter();
        let mut shown: Vec<&Event> = self
            .events
            .iter()
            .rev()
            .filter(|event| filter.matches(event))
            .take(SHOWN)
            .collect();
        shown.reverse();
        shown
    }

    fn visible_text(&self, scrub: bool) -> String {
        let redactions = crate::diagnostics::Redactions::from_environment();
        let events: Vec<Event> = self.visible().into_iter().cloned().collect();
        if scrub {
            debug_log::copy_text(&events, &redactions)
        } else {
            events
                .iter()
                .map(|event| debug_log::format_text(event, &redactions))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    fn toggle_subsystem(&mut self, name: &'static str) {
        if let Some(at) = self.subsystems.iter().position(|shown| *shown == name) {
            self.subsystems.remove(at);
        } else {
            self.subsystems.push(name);
        }
    }

    fn done(&mut self, message: String) {
        self.note = Some((message, false));
    }

    fn failed(&mut self, message: String) {
        self.note = Some((message, true));
    }
}

pub(super) fn page(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let view = &ui.log_view;
    let visible = view.visible();
    let streaming = if view.paused {
        tr("Paused")
    } else {
        tr("Streaming")
    };
    let header = row()
        .key("log-view-header")
        .gap(8.0)
        .center_y()
        .child(button(
            p,
            "log-view-back",
            tr("Back"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| ui.log_view.close(),
        ))
        .child(
            text(tr("Developer log"))
                .key("log-view-title")
                .font(p.k.ty.ui)
                .color(p.text()),
        )
        .child(
            text(tr_fill("{} events shown", &visible.len().to_string()))
                .key("log-view-count")
                .font(DESCRIPTION)
                .color(p.muted())
                .grow(1.0),
        )
        .child(
            text(streaming)
                .key("log-view-state")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
    let levels = pick_select(
        ui,
        p,
        "log-view-level",
        DebugLevel::ALL
            .iter()
            .map(|level| tr(level.label()))
            .collect(),
        DebugLevel::ALL
            .iter()
            .position(|level| *level == view.level)
            .unwrap_or(2),
        |ui, index| ui.log_view.level = DebugLevel::ALL[index],
        layer,
    );
    let controls = row()
        .key("log-view-controls")
        .gap(8.0)
        .center_y()
        .child(button(
            p,
            "log-view-pause",
            if view.paused {
                tr("Resume")
            } else {
                tr("Pause")
            },
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| ui.log_view.paused = !ui.log_view.paused,
        ))
        .child(levels)
        .child(
            TextInput::new(&view.search)
                .width(Some(200.0))
                .on_edit(|ui: &mut Ui| &mut ui.log_view.search)
                .build(p.k)
                .key("log-view-search"),
        )
        .child(button(
            p,
            "log-view-copy",
            tr("Copy visible"),
            ButtonKind::Secondary,
            |ui: &mut Ui, tx: &mut Tx| copy_visible(ui, tx),
        ))
        .child(button(
            p,
            "log-view-clear",
            tr("Clear"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| clear(ui),
        ))
        .child(button(
            p,
            "log-view-save",
            tr("Save to file"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| save(ui),
        ));
    let mut chips = row().key("log-view-subsystems").gap(6.0).center_y();
    for name in SUBSYSTEMS {
        let on = view.subsystems.contains(&name);
        chips = chips.child(button(
            p,
            subsystem_key(name),
            tr(subsystem_label(name)),
            if on {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            },
            move |ui: &mut Ui, _: &mut Tx| ui.log_view.toggle_subsystem(name),
        ));
    }
    let lines = if visible.is_empty() {
        tr("No events match yet").to_owned()
    } else {
        view.visible_text(false)
    };
    let body = col()
        .key("log-view-lines")
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .scroll(ScrollSpec {
            // Follows new events until paused or scrolled away.
            stick_bottom: !view.paused,
        })
        .child(
            text(lines)
                .key("log-view-text")
                .font(DESCRIPTION)
                .color(p.text())
                .selectable(true)
                .wrap(),
        );
    let mut page = col()
        .key("log-view")
        .grow(1.0)
        .min_h(0.0)
        .px(METRICS.gutter)
        .py(12.0)
        .gap(10.0)
        .child(header)
        .child(controls)
        .child(chips);
    if let Some((message, failed)) = &view.note {
        page = page.child(
            text(message.clone())
                .key("log-view-note")
                .font(DESCRIPTION)
                .color(if *failed { p.danger() } else { p.muted() })
                .wrap(),
        );
    }
    page.child(body)
}

fn subsystem_key(name: &str) -> &'static str {
    match name {
        "audio" => "log-sub-audio",
        "bluetooth" => "log-sub-bluetooth",
        "vad" => "log-sub-vad",
        "engine" => "log-sub-engine",
        "output" => "log-sub-output",
        "media" => "log-sub-media",
        "hotkeys" => "log-sub-hotkeys",
        "settings" => "log-sub-settings",
        _ => "log-sub-updater",
    }
}

fn subsystem_label(name: &str) -> &'static str {
    match name {
        "audio" => "Audio",
        "bluetooth" => "Bluetooth",
        "vad" => "Speech detection",
        "engine" => "Engine",
        "output" => "Output",
        "media" => "Media",
        "hotkeys" => "Hotkeys",
        "settings" => "Settings",
        _ => "Updater",
    }
}

fn copy_visible(ui: &mut Ui, tx: &mut Tx) {
    let text = ui.log_view.visible_text(true);
    if text.is_empty() {
        ui.log_view.failed(tr("Nothing to copy").to_owned());
        return;
    }
    let lines = text.lines().count().to_string();
    tx.copy(text);
    ui.log_view.done(tr_fill(
        "Copied {} lines, scrubbed of paths and keys",
        &lines,
    ));
}

fn clear(ui: &mut Ui) {
    ui.log_view.events.clear();
    if cfg!(test) {
        return;
    }
    // The buffer and the files go too, so a cleared view stays cleared.
    match debug_log::clear() {
        Ok(_) => ui.log_view.done(tr("Cleared the debug log").to_owned()),
        Err(error) => ui.log_view.failed(error),
    }
}

fn save(ui: &mut Ui) {
    if cfg!(test) {
        return;
    }
    let text = ui.log_view.visible_text(false);
    let result = crate::app_paths::logs_dir()
        .map_err(|error| format!("{error:#}"))
        .and_then(|dir| {
            std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            let path = dir.join(format!(
                "debug-view-{}.log",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.as_secs())
            ));
            std::fs::write(&path, text + "\n").map_err(|error| error.to_string())?;
            Ok(path)
        });
    match result {
        Ok(path) => ui
            .log_view
            .done(tr_fill("Saved to {}", &path.display().to_string())),
        Err(error) => ui.log_view.failed(tr_fill("Could not save: {}", &error)),
    }
}

#[cfg(test)]
impl LogView {
    /// Replaces what the view holds with `events` and detaches it from the
    /// process-wide ring, which other tests write to.
    pub(super) fn show_for_test(&mut self, events: Vec<Event>) {
        self.events = events.into();
        self.receiver = None;
        self.open = true;
    }

    /// Delivers `events` as if they streamed in.
    pub(super) fn stream_for_test(&mut self) -> std::sync::mpsc::Sender<Event> {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.receiver = Some(receiver);
        sender
    }
}
