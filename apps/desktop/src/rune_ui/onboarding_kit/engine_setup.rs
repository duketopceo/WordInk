//! The first run's engine and hotkey steps (F1): after the permissions, the
//! machine's recommendation with one alternative, then a masked field for a
//! Groq key or the Parakeet v3 download, then the dictation shortcut. The
//! state machine is `desktop::onboarding::Flow` (steps `Engine` and
//! `Hotkey`); this draws its pages over the window while the story waits
//! underneath, and runs what the buttons ask: saving the key through the
//! `keys` API (as `wordink keys set groq` does) and starting the download.

use rune::{El, TextEdit, Tx, col, edit_access, el, row, text};
use rune_kit::{Badge, Button, ButtonVariant, Progress, TextInput};

use super::super::controls::{ButtonKind, DESCRIPTION, Paint, PushSize, button, card, raised};
use super::super::general::shortcut_rows;
use super::super::icons::KEYBOARD;
use super::super::{Ui, onboarding};
use super::landing::{arrives, heading};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::desktop::onboarding::{EngineKind, EngineView, Input, Step};
use crate::hardware::gpu::{GpuStatus, GpuSummary};
use crate::hardware::{Choice, Probed, Reason, Recommendation};
use crate::keys::KeySource;
use crate::transcription::cloud::Provider;
use crate::transcription::models::{
    self as transcription_models, TranscriptionModelId, TranscriptionSelection,
};

const PAGE_W: f32 = 520.0;
const TOP_BAR: f32 = 48.0;

/// The window's side of the engine step.
pub(in crate::rune_ui) struct State {
    /// The pasted Groq key. Masked, cleared the moment it is saved, and never
    /// drawn.
    pub(in crate::rune_ui) key: TextEdit,
    /// Why the last save failed. Never holds the key.
    error: Option<String>,
    /// Where the saved key went.
    stored: Option<KeySource>,
    /// The hardware probe's answer, once it has one.
    pub(in crate::rune_ui) probed: Option<Probed>,
    /// Parakeet v3 and its language, once local was chosen: the first run's
    /// progress follows it before the service has saved the selection.
    local: Option<TranscriptionSelection>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            key: secure_field(),
            error: None,
            stored: None,
            probed: None,
            local: None,
        }
    }
}

fn secure_field() -> TextEdit {
    TextEdit::default().secure()
}

/// The selection the first run reads progress for: Parakeet v3 once local
/// was chosen, else the saved one.
pub(in crate::rune_ui) fn selection(ui: &Ui) -> TranscriptionSelection {
    ui.engine_setup
        .local
        .clone()
        .unwrap_or_else(|| ui.settings.transcription.clone())
}

/// The local model the probe recommends (Parakeet v3, or the same model on
/// the Neural Engine where that is ready), else Parakeet v3.
fn local_model(ui: &Ui) -> TranscriptionModelId {
    ui.engine_setup
        .probed
        .as_ref()
        .map(Probed::recommendation)
        .into_iter()
        .flat_map(|r| [r.primary, r.alternative])
        .find_map(|choice| match choice {
            Choice::Local(id) => id.parse().ok(),
            Choice::Cloud(_) => None,
        })
        .unwrap_or(TranscriptionModelId::ParakeetV3)
}

/// The local model in the person's language where it speaks it, else English.
fn local_selection(model: TranscriptionModelId) -> TranscriptionSelection {
    let definition = transcription_models::definition(model);
    let preferred = crate::desktop::i18n::preferred_language();
    // Parakeet v3 cannot detect a language, so a locale it lacks (or none at
    // all, as in a bare environment) falls to its default, English.
    let language = definition.language_for(&preferred).to_string();
    TranscriptionSelection { model, language }
}

fn engine(ui: &Ui) -> Option<EngineView> {
    ui.tour.as_ref().and_then(|tour| tour.flow.view().engine)
}

/// The tour's step when it is one of these pages.
fn step(ui: &Ui) -> Option<Step> {
    ui.tour
        .as_ref()
        .map(|tour| tour.flow.step())
        .filter(|step| matches!(step, Step::Engine | Step::Hotkey))
}

// ---------------------------------------------------------------------------
// Handlers

pub(in crate::rune_ui) fn choose_local(ui: &mut Ui) {
    let model = local_model(ui);
    let selection = local_selection(model);
    ui.engine_setup.local = Some(selection.clone());
    if let Some(live) = ui.first_run.as_mut().and_then(|s| s.live.as_mut()) {
        live.set_choice(selection);
    }
    ui.update(|settings| {
        settings.engine = crate::transcription::cloud::EngineChoice::Local;
        // The Neural Engine model is a Labs engine: choosing it here turns it on.
        if model == TranscriptionModelId::ParakeetV3Ane {
            settings.labs.neural_engine = true;
        }
    });
    onboarding::input(ui, Input::ChooseEngine(EngineKind::Local));
}

pub(in crate::rune_ui) fn choose_cloud(ui: &mut Ui) {
    onboarding::input(ui, Input::ChooseEngine(EngineKind::Cloud));
    // A key already set (environment, keyring or file) needs no pasting.
    if ui
        .engine_setup
        .probed
        .as_ref()
        .is_some_and(|probed| probed.has_groq_key)
    {
        use_cloud(ui);
    }
}

fn use_cloud(ui: &mut Ui) {
    ui.update(|settings| settings.engine = crate::transcription::cloud::EngineChoice::Groq);
    crate::transcription::cloud::refresh_key_in_background();
    onboarding::input(ui, Input::KeyStored);
}

/// Saves the pasted key like `wordink keys set groq`, clears the field, and
/// tells the flow. A scene or test saves nothing.
fn save_key(ui: &mut Ui, _: &mut Tx) {
    let mut buffer = ui.engine_setup.key.text.clone();
    let live = ui.live();
    let result = crate::desktop::onboarding::submit_key(&mut buffer, |account, key| {
        if live {
            crate::keys::store(account, key)
        } else {
            Ok(KeySource::Keyring)
        }
    });
    match result {
        Ok(source) => {
            ui.engine_setup.key = secure_field();
            ui.engine_setup.error = None;
            ui.engine_setup.stored = Some(source);
            use_cloud(ui);
        }
        Err(error) => ui.engine_setup.error = Some(format!("{error:#}")),
    }
}

fn continue_step(ui: &mut Ui, _: &mut Tx) {
    onboarding::input(ui, Input::Continue);
}

// ---------------------------------------------------------------------------
// Pages

/// The page that replaces the first run's story while the tour is at one of
/// these steps.
pub(in crate::rune_ui) fn overlay(ui: &Ui, p: Paint) -> Option<El> {
    let step = step(ui)?;
    let page = match step {
        Step::Engine => engine_page(ui, p),
        _ => hotkey_page(ui, p),
    };
    let top = super::super::title_bar(row().key("top-bar").w_full().h(TOP_BAR).px(20.0).center_y());
    Some(
        col()
            .key("first-run")
            .role("dialog")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(p.k.c.bg_deep)
            .on_press(|_: &mut Ui| {})
            .child(top)
            .child(
                col()
                    .key("middle")
                    .grow(1.0)
                    .shrink()
                    .w_full()
                    .center()
                    .pb(TOP_BAR)
                    .child(page),
            ),
    )
}

/// The probe's one-line summary of this computer: the GPU by name and
/// whether it is ready, the Neural Engine where it is, then the memory.
pub(crate) fn machine_line(probed: &Probed) -> String {
    let probe = &probed.probe;
    let mut line = gpu_line(&probe.gpu);
    if probe.neural_engine {
        line.push(' ');
        line.push_str(tr("Apple Neural Engine ready."));
    }
    if probe.ram_gb > 0.0 {
        line.push(' ');
        line.push_str(&tr_fill("Memory: {} GB.", &format!("{:.0}", probe.ram_gb)));
    }
    line
}

/// The GPU sentence, such as "NVIDIA GeForce RTX 4060 Laptop GPU: GPU
/// acceleration ready." Only a GPU without a Vulkan driver asks for an
/// install; an unknown name falls back to a sentence without one.
pub(crate) fn gpu_line(gpu: &GpuSummary) -> String {
    let named = |template: &str, fallback: &str| match &gpu.name {
        Some(name) => tr_named(template, &[("gpu", name)]),
        None => tr(fallback).to_string(),
    };
    match gpu.status {
        GpuStatus::Ready => named("{gpu}: GPU acceleration ready.", "GPU acceleration ready."),
        GpuStatus::DriverFound => named(
            "{gpu}: graphics driver found. WordInk uses the GPU once the model loads.",
            "Graphics driver found. WordInk uses the GPU once the model loads.",
        ),
        GpuStatus::NeedsDriver => {
            let mut line = named(
                "{gpu}: its graphics driver has no Vulkan support.",
                "This GPU's graphics driver has no Vulkan support.",
            );
            line.push(' ');
            line.push_str(&match gpu.vendor.name() {
                Some(vendor) => tr_fill(
                    "Install or update the {} graphics driver to transcribe on it.",
                    vendor,
                ),
                None => {
                    tr("Install or update the graphics driver to transcribe on it.").to_string()
                }
            });
            line
        }
        GpuStatus::CpuBuild => tr("This version of WordInk transcribes on the CPU.").to_string(),
        GpuStatus::Software => {
            tr("Only a software graphics driver here; transcription runs on the CPU.").to_string()
        }
        GpuStatus::None => tr("No GPU found; transcription runs on the CPU.").to_string(),
    }
}

pub(crate) fn reason_copy(reason: Reason) -> &'static str {
    match reason {
        Reason::HasKey => tr("A Groq key is already set on this computer."),
        Reason::NoAvx2 => tr("This processor can't run local models fast enough."),
        Reason::NoGpu => tr("There is no GPU here to run a local model on."),
        Reason::NeedsDriver => {
            tr("Until the GPU has a Vulkan driver, a local model would run slowly on the CPU.")
        }
        Reason::SoftwareGpu => {
            tr("Only a software graphics driver is available, which is too slow for a local model.")
        }
        Reason::LowMemory => tr("This computer has little memory for a local model."),
        Reason::FitsLocal => {
            tr("This computer's GPU and memory run it well, and nothing leaves it.")
        }
        Reason::NeuralEngine => tr(
            "The Neural Engine runs it fast and leaves the GPU free, and nothing leaves this computer.",
        ),
    }
}

fn choice_kind(choice: Choice) -> EngineKind {
    match choice {
        Choice::Cloud(_) => EngineKind::Cloud,
        Choice::Local(_) => EngineKind::Local,
    }
}

fn engine_page(ui: &Ui, p: Paint) -> El {
    let view = engine(ui);
    let recommendation = view.and_then(|view| view.recommendation);
    let chosen = view.and_then(|view| view.chosen);
    let can_continue = view.is_some_and(|view| view.can_continue);
    let lead = ui
        .engine_setup
        .probed
        .as_ref()
        .map_or_else(|| tr("Checking this computer…").to_string(), machine_line);
    let (primary, alternative) = match recommendation {
        Some(Recommendation {
            primary,
            alternative,
            ..
        }) => (primary, alternative),
        // Until the probe answers, local first as the catalogue has it.
        None => (
            Choice::Local(crate::hardware::PARAKEET_V3),
            Choice::Cloud(Provider::Groq),
        ),
    };
    let reason = recommendation.map(|r| reason_copy(r.reason));
    let mut page = col()
        .key("engine")
        .w(PAGE_W)
        .gap(18.0)
        .items_center()
        .child(heading(
            p,
            "engine-head",
            tr("Choose how WordInk transcribes"),
            &lead,
        ))
        .child(option_card(
            p,
            primary,
            Some(tr("Recommended")),
            reason,
            chosen == Some(choice_kind(primary)),
        ))
        .child(option_card(
            p,
            alternative,
            None,
            None,
            chosen == Some(choice_kind(alternative)),
        ));
    page = match chosen {
        Some(EngineKind::Cloud) => page.child(cloud_detail(ui, p, view)),
        Some(EngineKind::Local) => page.child(local_detail(ui, p)),
        None => page,
    };
    page.child(row().key("action").h(32.0).center().child(if can_continue {
        Button::new(tr("Continue"))
            .variant(ButtonVariant::Contrast)
            .on_press_tx(continue_step)
            .build_push(p.k)
            .key("continue")
            .transition(arrives())
    } else {
        el().key("none")
    }))
    .child(finish_later(p))
}

/// "Finish later": on to the app, which keeps a "Set up dictation" row.
fn finish_later(p: Paint) -> El {
    row().key("later").h(32.0).center().child(
        Button::new(tr("Finish later"))
            .variant(ButtonVariant::Ghost)
            .on_press(super::live::finish_later)
            .build_push(p.k)
            .key("finish-later"),
    )
}

fn option_title(choice: Choice) -> String {
    match choice {
        Choice::Cloud(provider) => tr_fill("{} in the cloud", provider.name()),
        Choice::Local(id) => {
            let model = id
                .parse::<TranscriptionModelId>()
                .map(transcription_models::definition);
            match model {
                Ok(model) => tr_fill("{} on this computer", model.name),
                Err(_) => tr("On this computer").to_string(),
            }
        }
    }
}

fn option_blurb(choice: Choice) -> String {
    match choice {
        Choice::Cloud(provider) => tr_fill(
            "Fast on any computer. Each recording is sent to {} to be transcribed. Needs an API key.",
            provider.name(),
        ),
        Choice::Local(id) => {
            let size = id
                .parse::<TranscriptionModelId>()
                .map(|id| transcription_models::definition(id).size_label())
                .unwrap_or_default();
            tr_fill(
                "Private and works offline. Downloads {} once, from huggingface.co.",
                &size,
            )
        }
    }
}

fn option_card(
    p: Paint,
    choice: Choice,
    tag: Option<&'static str>,
    reason: Option<&'static str>,
    selected: bool,
) -> El {
    let c = &p.k.c;
    let key = match choice {
        Choice::Cloud(_) => "option-cloud",
        Choice::Local(_) => "option-local",
    };
    let mut title = row().key("title-row").gap(8.0).center_y().child(
        text(option_title(choice))
            .key("title")
            .font(p.k.ty.ui_medium)
            .color(p.text()),
    );
    if let Some(tag) = tag {
        title = title.child(Badge::new(tag).build(p.k).key("recommended"));
    }
    let mut body = col()
        .key("body")
        .gap(4.0)
        .grow(1.0)
        .shrink()
        .child(title)
        .child(
            text(option_blurb(choice))
                .key("blurb")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        );
    if let Some(reason) = reason {
        body = body.child(
            text(reason)
                .key("reason")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap(),
        );
    }
    let mut card = raised(p, key)
        .w_full()
        .padding(14.0)
        .child(body)
        .role("button")
        .label(option_title(choice))
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .hover(c.hover)
        .press(c.pressed);
    if selected {
        card = card.outline(1.5, 0.0, c.border_focus);
    }
    match choice {
        Choice::Cloud(_) => card.on_press(choose_cloud),
        Choice::Local(_) => card.on_press(choose_local),
    }
}

fn cloud_detail(ui: &Ui, p: Paint, view: Option<EngineView>) -> El {
    let saved = view.is_some_and(|view| view.key_saved);
    let mut detail = col().key("cloud-detail").w_full().gap(8.0);
    if saved {
        let note = match ui.engine_setup.stored {
            Some(source) => tr_fill("Key saved in the {}. It isn't shown again.", source.label()),
            None => tr("A Groq key is already set on this computer.").to_string(),
        };
        return detail.child(
            text(note)
                .key("key-saved")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        );
    }
    detail = detail.child(
        row()
            .key("key-row")
            .gap(8.0)
            .center_y()
            .child(
                TextInput::new(&ui.engine_setup.key)
                    .secure()
                    .placeholder(tr("Paste your Groq API key"))
                    .width(None)
                    .on_edit(edit_access(|ui: &mut Ui| &mut ui.engine_setup.key))
                    .on_submit(save_key)
                    .build(p.k)
                    .key("key-field")
                    .grow(1.0)
                    .shrink(),
            )
            .child(button(
                p,
                "save-key",
                tr("Save key"),
                ButtonKind::Primary,
                save_key,
            )),
    );
    if let Some(error) = &ui.engine_setup.error {
        detail = detail.child(
            text(error.clone())
                .key("key-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap(),
        );
    } else {
        detail = detail.child(
            text(tr(
                "Get a key at console.groq.com. It is stored in your OS keyring, never in settings.",
            ))
            .key("key-help")
            .font(DESCRIPTION)
            .color(p.faint())
            .wrap(),
        );
    }
    detail
}

fn local_detail(ui: &Ui, p: Paint) -> El {
    let (installed, progress) = onboarding::progress(ui);
    let mut detail = col().key("local-detail").w_full().gap(8.0);
    if installed {
        return detail.child(
            text(tr(
                "Downloaded. WordInk is ready to transcribe on this computer.",
            ))
            .key("download-done")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap(),
        );
    }
    if progress.failed {
        let why = super::super::model_import::download_failure(ui)
            .unwrap_or_else(|| tr("The download failed.").to_string());
        return detail
            .child(
                text(why)
                    .key("download-error")
                    .font(DESCRIPTION)
                    .color(p.danger())
                    .wrap(),
            )
            .child(button(
                p,
                "retry-download",
                tr("Try again"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    onboarding::input(
                        ui,
                        Input::Grant(crate::desktop::onboarding::SetupItem::Model),
                    )
                },
            ));
    }
    let bar = Progress::new(progress.downloading).label(tr("Downloading"));
    let bar = if progress.downloading.is_some() {
        bar.show_value()
    } else {
        bar
    };
    detail = detail.child(bar.build(p.k).key("download-progress"));
    detail.child(
        text(tr("You can go on while it downloads."))
            .key("download-note")
            .font(DESCRIPTION)
            .color(p.faint()),
    )
}

fn hotkey_page(ui: &Ui, p: Paint) -> El {
    let rows = shortcut_rows(
        ui,
        p,
        "hotkey",
        &KEYBOARD,
        tr("Dictation shortcut"),
        super::super::ShortcutTarget::Dictation,
        Some(tr("Hold it to speak, release to type")),
    );
    col()
        .key("hotkey-page")
        .w(PAGE_W)
        .gap(18.0)
        .items_center()
        .child(heading(
            p,
            "hotkey-head",
            tr("Your dictation shortcut"),
            tr("Keep this one, or choose another. You can change it later in General."),
        ))
        .child(card(p, "hotkey-card", rows).w_full())
        .children(other_tools_note(ui, p))
        .child(
            row().key("action").h(32.0).center().child(
                Button::new(tr("Continue"))
                    .variant(ButtonVariant::Contrast)
                    .on_press_tx(continue_step)
                    .build_push(p.k)
                    .key("continue")
                    .transition(arrives()),
            ),
        )
}

/// Another dictation tool on this computer, named, so two tools don't both
/// start on one shortcut.
fn other_tools_note(ui: &Ui, p: Paint) -> Option<El> {
    let tools = &ui.engine_setup.probed.as_ref()?.other_tools;
    let names = tools
        .iter()
        .map(|tool| tool.name())
        .collect::<Vec<_>>()
        .join(", ");
    (!tools.is_empty()).then(|| {
        text(tr_named(
            "{tool} is also installed here. If it listens for the same shortcut, change one of them.",
            &[("tool", &names)],
        ))
        .key("other-tools")
        .font(DESCRIPTION)
        .color(p.muted())
        .wrap()
    })
}

#[cfg(test)]
mod tests {
    use super::gpu_line;
    use crate::hardware::gpu::{GpuStatus, GpuSummary, Vendor};

    fn summary(status: GpuStatus, name: Option<&str>, vendor: Vendor) -> GpuSummary {
        GpuSummary {
            status,
            name: name.map(str::to_string),
            vendor,
        }
    }

    #[test]
    fn the_gpu_line_names_the_gpu_and_asks_for_a_driver_only_when_one_is_missing() {
        let rtx = Some("NVIDIA GeForce RTX 4060 Laptop GPU");
        assert_eq!(
            gpu_line(&summary(GpuStatus::Ready, rtx, Vendor::Nvidia)),
            "NVIDIA GeForce RTX 4060 Laptop GPU: GPU acceleration ready."
        );
        assert_eq!(
            gpu_line(&summary(GpuStatus::Ready, None, Vendor::Other)),
            "GPU acceleration ready."
        );
        let found = gpu_line(&summary(GpuStatus::DriverFound, rtx, Vendor::Nvidia));
        assert!(found.starts_with("NVIDIA GeForce RTX 4060 Laptop GPU: graphics driver found."));
        for status in [
            GpuStatus::Ready,
            GpuStatus::DriverFound,
            GpuStatus::CpuBuild,
            GpuStatus::Software,
            GpuStatus::None,
        ] {
            let line = gpu_line(&summary(status, rtx, Vendor::Nvidia));
            assert!(!line.contains("Install"), "{status:?}: {line}");
        }
        assert_eq!(
            gpu_line(&summary(
                GpuStatus::NeedsDriver,
                Some("AMD GPU"),
                Vendor::Amd
            )),
            "AMD GPU: its graphics driver has no Vulkan support. Install or update the AMD graphics driver to transcribe on it."
        );
        assert_eq!(
            gpu_line(&summary(GpuStatus::NeedsDriver, None, Vendor::Other)),
            "This GPU's graphics driver has no Vulkan support. Install or update the graphics driver to transcribe on it."
        );
        assert_eq!(
            gpu_line(&summary(GpuStatus::None, None, Vendor::Other)),
            "No GPU found; transcription runs on the CPU."
        );
    }
}
