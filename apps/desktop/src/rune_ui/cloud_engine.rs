//! The cloud engine rows in General › Advanced › Dictation: which engine
//! dictation uses (Auto, the local model, or a cloud provider) and, for a
//! provider with a choice of models, which one. Keys are never shown or
//! stored here; they are set with `wordink keys set <provider>`.

use std::collections::HashSet;
use std::sync::Mutex;

use rune::El;

use super::controls::{CardRow, Paint, icon_row};
use super::general::quiet_select;
use super::icons::WAVE;
use super::{Menu, Ui};
use crate::desktop::i18n::{tr, tr_fill};
use crate::transcription::cloud::{self, EngineChoice, LocalFallback, Provider};

/// A `'static` copy of a model name for the select, which takes static
/// labels. One copy per distinct name, so redrawing leaks nothing.
fn interned(name: &str) -> &'static str {
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|error| error.into_inner());
    let names = names.get_or_insert_with(HashSet::new);
    if let Some(known) = names.get(name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// What General shows for the engine: the provider dictation runs on, or
/// the local model, and how it was chosen.
pub(super) fn active_engine_label(ui: &Ui, local_model: &str) -> String {
    let choice = ui.settings.engine;
    match (cloud::route_for_choice(choice), choice) {
        (Some(provider), EngineChoice::Auto) => tr_fill("{} (auto)", provider.name()),
        (Some(provider), _) => provider.name().to_owned(),
        (None, _) => tr_fill("{} (local)", local_model),
    }
}

/// The model dictation transcribes with when it routes to the cloud: the
/// provider and its model, such as "Groq · whisper-large-v3-turbo".
/// `None` when the local model transcribes.
pub(super) fn active_cloud_model(ui: &Ui) -> Option<String> {
    let provider = cloud::route_for_choice(ui.settings.engine)?;
    let label = match provider {
        // The gateway picks its own model.
        Provider::Gateway => provider.name().to_owned(),
        _ => format!(
            "{} \u{b7} {}",
            provider.name(),
            cloud::model_for(provider, ui.settings.engine_model.as_deref())
        ),
    };
    Some(label)
}

/// The engine row, and the model row under it when the engine has models
/// to choose between.
pub(super) fn rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let choice = ui.settings.engine;
    let labels: Vec<&'static str> = EngineChoice::ALL.iter().map(|c| tr(c.label())).collect();
    let selected = EngineChoice::ALL
        .iter()
        .position(|c| *c == choice)
        .unwrap_or(0);
    let engine = quiet_select(
        ui,
        p,
        Menu::Pick("cloud-engine"),
        "cloud-engine-control".to_owned(),
        labels,
        selected,
        |ui, index| {
            let choice = EngineChoice::ALL[index];
            if ui.settings.engine != choice {
                ui.update(|settings| settings.engine = choice);
            }
        },
        layer,
    );
    let note = match choice.provider() {
        Some(Provider::Gateway) => {
            tr("Set gateway_url in settings.json and the token with `wordink keys set gateway`")
                .to_owned()
        }
        Some(provider) => tr_fill("Set the key with `wordink keys set {}`", provider.id()),
        None => tr("Audio leaves this computer only when a cloud engine is used").to_owned(),
    };
    let mut rows: Vec<CardRow> = vec![
        icon_row(
            p,
            "cloud-engine",
            &WAVE,
            tr("Cloud engine"),
            Some(&note),
            engine,
        )
        .into(),
    ];
    if matches!(choice, EngineChoice::Local | EngineChoice::Auto) {
        rows.push(fallback_row(ui, p, layer).into());
    }
    rows.push(super::openrouter_connect::settings_row(ui, p).into());
    if let Some(provider) = choice.provider() {
        let models = cloud::models_for(provider);
        if models.len() > 1 {
            let current = cloud::model_for(provider, ui.settings.engine_model.as_deref());
            let labels: Vec<&'static str> = models.iter().map(|(_, name)| interned(name)).collect();
            let selected = models
                .iter()
                .position(|(id, _)| *id == current)
                .unwrap_or(0);
            let ids: Vec<String> = models.into_iter().map(|(id, _)| id).collect();
            let model = quiet_select(
                ui,
                p,
                Menu::Pick("cloud-model"),
                "cloud-model-control".to_owned(),
                labels,
                selected,
                move |ui, index| {
                    if let Some(id) = ids.get(index) {
                        let id = id.clone();
                        ui.update(|settings| settings.engine_model = Some(id));
                    }
                },
                layer,
            );
            let description = (provider == Provider::OpenRouter).then(|| {
                tr("Every OpenRouter model that takes audio in; refreshed daily").to_owned()
            });
            rows.push(
                icon_row(
                    p,
                    "cloud-model",
                    &WAVE,
                    tr("Cloud model"),
                    description.as_deref(),
                    model,
                )
                .into(),
            );
        }
    }
    rows.extend(meeting_rows(ui, p, layer));
    rows
}

/// "If local fails, use": the opt-in cloud retry for a failed local
/// dictation. None keeps the audio on this computer.
fn fallback_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> impl Into<CardRow> {
    let current = ui.settings.local_cloud_fallback;
    let labels: Vec<&'static str> = LocalFallback::ALL.iter().map(|c| tr(c.label())).collect();
    let selected = LocalFallback::ALL
        .iter()
        .position(|c| *c == current)
        .unwrap_or(0);
    let control = quiet_select(
        ui,
        p,
        Menu::Pick("local-fallback"),
        "local-fallback-control".to_owned(),
        labels,
        selected,
        |ui, index| {
            let choice = LocalFallback::ALL[index];
            if ui.settings.local_cloud_fallback != choice {
                ui.update(|settings| settings.local_cloud_fallback = choice);
            }
        },
        layer,
    );
    let note = match current.provider() {
        Some(provider) => tr_fill("Set the key with `wordink keys set {}`", provider.id()),
        None => tr("Off: a failed local dictation never leaves this computer").to_owned(),
    };
    icon_row(
        p,
        "local-fallback",
        &WAVE,
        tr("If local fails, use"),
        Some(&note),
        control,
    )
}

/// The engines a meeting can be given: the local model (the default, and no
/// setting) or one cloud provider.
const MEETING_ENGINES: [Option<EngineChoice>; 6] = [
    None,
    Some(EngineChoice::Groq),
    Some(EngineChoice::Openrouter),
    Some(EngineChoice::Openai),
    Some(EngineChoice::Deepgram),
    Some(EngineChoice::Gateway),
];

/// The models a meeting can use at `provider`: OpenRouter's transcription
/// models (a meeting wants timestamps), else the provider's own list.
fn meeting_models(provider: Provider) -> Vec<(String, String)> {
    match provider {
        Provider::OpenRouter => cloud::transcription_models()
            .into_iter()
            .map(|model| (model.id, model.name))
            .collect(),
        other => cloud::models_for(other),
    }
}

/// The meeting engine row, and under it the model row when the provider has
/// models to choose between. Meetings use the local model unless a cloud
/// engine is chosen here, whatever dictation uses.
fn meeting_rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let choice = ui
        .settings
        .meeting_engine
        .filter(|c| c.provider().is_some());
    let labels: Vec<&'static str> = MEETING_ENGINES
        .iter()
        .map(|c| tr(c.unwrap_or(EngineChoice::Local).label()))
        .collect();
    let selected = MEETING_ENGINES
        .iter()
        .position(|c| *c == choice)
        .unwrap_or(0);
    let engine = quiet_select(
        ui,
        p,
        Menu::Pick("meeting-engine"),
        "meeting-engine-control".to_owned(),
        labels,
        selected,
        |ui, index| {
            let choice = MEETING_ENGINES[index];
            if ui.settings.meeting_engine != choice {
                ui.update(|settings| {
                    settings.meeting_engine = choice;
                    settings.meeting_engine_model = None;
                });
            }
        },
        layer,
    );
    let note = match choice.and_then(EngineChoice::provider) {
        Some(Provider::Gateway) => {
            tr("Set gateway_url in settings.json and the token with `wordink keys set gateway`")
                .to_owned()
        }
        Some(provider) => tr_fill("Set the key with `wordink keys set {}`", provider.id()),
        None => tr("Meetings stay on this computer unless you choose a cloud engine").to_owned(),
    };
    let mut rows: Vec<CardRow> = vec![
        icon_row(
            p,
            "meeting-engine",
            &WAVE,
            tr("Meeting engine"),
            Some(&note),
            engine,
        )
        .into(),
    ];
    if choice != Some(EngineChoice::Openrouter) {
        rows.push(super::openrouter_connect::meeting_offer_row(ui, p).into());
    }
    if let Some(provider) = choice.and_then(EngineChoice::provider) {
        let models = meeting_models(provider);
        if models.len() > 1 {
            let current =
                cloud::meeting_model_for(provider, ui.settings.meeting_engine_model.as_deref());
            let labels: Vec<&'static str> = models.iter().map(|(_, name)| interned(name)).collect();
            let selected = models
                .iter()
                .position(|(id, _)| *id == current)
                .unwrap_or(0);
            let ids: Vec<String> = models.into_iter().map(|(id, _)| id).collect();
            let model = quiet_select(
                ui,
                p,
                Menu::Pick("meeting-model"),
                "meeting-model-control".to_owned(),
                labels,
                selected,
                move |ui, index| {
                    if let Some(id) = ids.get(index) {
                        let id = id.clone();
                        ui.update(|settings| settings.meeting_engine_model = Some(id));
                    }
                },
                layer,
            );
            rows.push(icon_row(p, "meeting-model", &WAVE, tr("Meeting model"), None, model).into());
        }
    }
    rows
}
