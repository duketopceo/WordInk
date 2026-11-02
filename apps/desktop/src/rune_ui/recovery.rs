//! Failed dictations in History (R11, R26): the kept clips listed beside
//! the retained entries, Retry on the engine that failed or another one, the
//! ask before audio recorded for a local model goes to a cloud provider, and
//! the recovered text with Copy and Paste. A retry inserts nothing by itself.

use rune::{El, Tx, col, row, text};
use rune_kit::{Select, SelectAppearance};

use super::controls::{ButtonKind, Paint, button};
use super::history::{HistoryPane, Recovered, Retrying};
use super::list_detail::{self as ld, Item};
use super::lists::error_panel;
use super::{Menu, Ui};
use crate::desktop::i18n::{event_age, tr, tr_fill};
use crate::transcription::cloud::{self, Provider};
use crate::transcription::recover::{self, FailedClip, RetryError, Target};

/// A failed dictation's row in the list.
pub(super) fn failed_item(p: Paint, pane: &HistoryPane, clip: &FailedClip) -> El {
    let name = clip.file_name();
    ld::item(
        p,
        Item {
            key: format!("history-failed{}", clip.timestamp_ms),
            title: tr("Failed dictation").to_owned(),
            meta: format!("{} · {}", tr("Kept for retry"), clip.origin_label()),
            meta_ink: None,
            time: event_age(clip.timestamp_ms),
            hover: Vec::new(),
            selected: pane.selected_failed.as_deref() == Some(name.as_str()),
        },
        move |ui: &mut Ui| {
            ui.history.selected_failed = Some(name.clone());
            ui.history.selected = None;
            ui.history.recovered = None;
            ui.history.confirm = None;
            ui.history.retry_error = None;
            ui.history.retry_engine = None;
            ui.history.clear_armed = false;
        },
    )
}

fn delete_clip(ui: &mut Ui, name: &str) {
    if let Some(dir) = cloud::failed_clips_dir().filter(|_| ui.live())
        && let Err(error) = recover::delete(&dir, name)
    {
        ui.history.error = Some(error.to_string());
    }
    ui.history.failed.retain(|clip| clip.file_name() != name);
    ui.history.selected_failed = None;
    ui.history.confirm = None;
    ui.history.reload();
}

/// Starts a retry of `name` on the chosen engine. A clip recorded for a
/// local model asks first: nothing is sent until `confirmed`.
fn retry(ui: &mut Ui, name: &str, confirmed: bool) {
    let Some(clip) = ui
        .history
        .failed
        .iter()
        .find(|clip| clip.file_name() == name)
        .cloned()
    else {
        return;
    };
    if recover::needs_confirmation(&clip.origin) && !confirmed {
        ui.history.confirm = Some(name.to_owned());
        return;
    }
    ui.history.confirm = None;
    ui.history.retry_error = None;
    let provider = ui.history.engine_for(&clip);
    let target = Target {
        provider,
        model: cloud::selected_model(provider),
        gateway_url: cloud::gateway_url(),
    };
    let (sender, result) = std::sync::mpsc::channel();
    // Fixtures send nothing: the request is the live window's alone.
    let live = ui.live();
    ui.history.retrying = Some(Retrying {
        clip: name.to_owned(),
        result,
        fixture_sender: (!live).then(|| sender.clone()),
    });
    if !live {
        return;
    }
    let Some(dir) = cloud::failed_clips_dir() else {
        let _ = sender.send(Err(RetryError::Failed("no folder for kept clips".into())));
        return;
    };
    let history = ui.history.store().cloned();
    let spawned = std::thread::Builder::new()
        .name("retry-dictation".into())
        .spawn(move || {
            let _ = sender.send(recover::retry(&dir, &clip, &target, true, history.as_ref()));
        });
    if spawned.is_err() {
        ui.history.retrying = None;
        ui.history.retry_error = Some("could not start the retry".into());
    }
}

pub(super) fn failed_detail(ui: &Ui, p: Paint, clip: &FailedClip, layer: &mut Vec<El>) -> El {
    let pane = &ui.history;
    let name = clip.file_name();
    let engine = pane.engine_for(clip);
    let retrying = pane.retrying.as_ref().is_some_and(|r| r.clip == name);
    let mut body = col().key("detail-body").gap(16.0).child(
        text(tr(
            "This dictation could not be transcribed. Its audio is kept so you can try again; nothing was inserted.",
        ))
        .key("failed-explain")
        .font(p.k.ty.body)
        .color(p.text())
        .wrap(),
    );
    if let Some(error) = &pane.retry_error {
        body = body.child(error_panel(p, "retry-error", tr("Retry failed"), error));
    }
    let facts = vec![
        (tr("Failed on"), clip.origin_label()),
        (tr("Recorded"), event_age(clip.timestamp_ms)),
    ];
    body = body.child(ld::facts(p, facts));

    if retrying {
        body = body.child(
            text(tr("Retrying…"))
                .key("retrying")
                .font(p.k.ty.ui_medium)
                .color(p.muted()),
        );
    } else if pane.confirm.as_deref() == Some(name.as_str()) {
        let target = tr_fill("Send to {}", engine.name());
        body = body.child(
            col()
                .key("history-confirm")
                .gap(10.0)
                .px(12.0)
                .py(10.0)
                .radius(p.k.radius.md)
                .fill(p.k.c.layer_01)
                .child(
                    text(tr_fill(
                        "This audio was recorded for a local model and has not left this computer. {} will receive the recording.",
                        engine.name(),
                    ))
                    .key("confirm-copy")
                    .font(super::controls::DESCRIPTION)
                    .color(p.text())
                    .wrap(),
                )
                .child(
                    row()
                        .key("confirm-actions")
                        .gap(8.0)
                        .child(button(
                            p,
                            "history-confirm-send",
                            &target,
                            ButtonKind::Danger,
                            {
                                let name = name.clone();
                                move |ui: &mut Ui, _| retry(ui, &name, true)
                            },
                        ))
                        .child(button(
                            p,
                            "history-confirm-cancel",
                            tr("Cancel"),
                            ButtonKind::Secondary,
                            |ui: &mut Ui, _| ui.history.confirm = None,
                        )),
                ),
        );
    } else {
        let labels: Vec<&str> = Provider::ALL
            .iter()
            .map(|provider| provider.name())
            .collect();
        let picker = Select::new("history-retry-engine", &labels)
            .appearance(SelectAppearance::Inline)
            .current(Some(engine.name()))
            .open(ui.open == Some(Menu::RetryEngine))
            .on_toggle(|ui: &mut Ui| {
                ui.open = (ui.open != Some(Menu::RetryEngine)).then_some(Menu::RetryEngine);
            })
            .on_close(|ui: &mut Ui| ui.open = None)
            .on_choose(|ui: &mut Ui, choice: &str, _: &mut Tx| {
                ui.open = None;
                ui.history.retry_engine = Provider::ALL.into_iter().find(|p| p.name() == choice);
            })
            .build(p.k, layer);
        let label = if Provider::from_id(&clip.origin) == Some(engine) {
            tr("Retry").to_owned()
        } else {
            tr_fill("Retry with {}", engine.name())
        };
        body = body.child(
            row()
                .key("retry-actions")
                .gap(8.0)
                .center_y()
                .child(button(p, "history-retry", &label, ButtonKind::Primary, {
                    let name = name.clone();
                    move |ui: &mut Ui, _| retry(ui, &name, false)
                }))
                .child(
                    text(tr("on"))
                        .key("retry-on")
                        .font(super::controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(picker),
        );
    }
    col()
        .key("history-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, tr("Failed dictation")),
            vec![ld::meta_text(p, &event_age(clip.timestamp_ms))],
            vec![ld::ghost_icon(
                p,
                "history-delete-clip",
                ld::TRASH_ICON,
                tr("Delete"),
                move |ui: &mut Ui, _| delete_clip(ui, &name),
            )],
        ))
        .child(ld::card_scroll("detail-scroll", body))
}

/// What a retry recovered, with Copy and Paste. Neither runs by itself.
pub(super) fn recovered_detail(p: Paint, recovered: &Recovered) -> El {
    let copy_text = recovered.text.clone();
    let paste_text = recovered.text.clone();
    let mut body = col()
        .key("detail-body")
        .gap(16.0)
        .child(
            text(recovered.text.clone())
                .key("recovered-text")
                .font(p.k.ty.body)
                .color(p.text())
                .wrap(),
        )
        .child(
            row()
                .key("recovered-actions")
                .gap(8.0)
                .child(button(
                    p,
                    "history-recovered-copy",
                    tr("Copy"),
                    ButtonKind::Secondary,
                    move |_: &mut Ui, tx: &mut Tx| {
                        tx.copy(copy_text.clone());
                    },
                ))
                .child(button(
                    p,
                    "history-recovered-paste",
                    tr("Paste"),
                    ButtonKind::Primary,
                    move |ui: &mut Ui, tx: &mut Tx| {
                        // This window has the focus, so it can't type into
                        // another app: the text goes to the clipboard, and it is
                        // saved as the last dictation for Paste last.
                        tx.copy(paste_text.clone());
                        if let Some(recovered) = &mut ui.history.recovered {
                            recovered.pasted = true;
                        }
                    },
                )),
        );
    if recovered.pasted {
        body = body.child(
            text(tr(
                "Copied. Switch to the app and paste, or press your Paste last shortcut: it is saved as your last dictation.",
            ))
            .key("pasted-hint")
            .font(super::controls::DESCRIPTION)
            .color(p.muted())
            .wrap(),
        );
    }
    body = body.child(
        text(tr_fill(
            "Recovered with {}. Nothing was inserted.",
            &recovered.engine,
        ))
        .key("recovered-note")
        .font(super::controls::DESCRIPTION)
        .color(p.muted())
        .wrap(),
    );
    col()
        .key("history-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, tr("Recovered dictation")),
            Vec::new(),
            Vec::new(),
        ))
        .child(ld::card_scroll("detail-scroll", body))
}
