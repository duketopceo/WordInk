//! Microphone capsule button on the try screen (`t421`), sized and styled so
//! it pairs cleanly beside `[ ⌥  Change shortcut ]` on the same horizontal
//! spine.
//!
//! The real first run names the input Hex records from ([`pick`]), from the
//! same listing and `microphone` setting as Settings › General; the stories
//! show [`DEMO_NAME`].

use std::collections::BTreeMap;

use rune::{El, Spring, Transition, icon, row, text};

use super::super::controls::{DESCRIPTION, Paint};
use crate::audio::InputDevices;
use crate::audio::sources::DeviceKind;
use crate::desktop::i18n::tr;

const H: f32 = 28.0;
/// The stories' and playground's microphone, never the real first run's.
pub(crate) const DEMO_NAME: &str = "MacBook Pro Microphone";

/// Where the input listing stands.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Listing<'a> {
    /// Not listed yet.
    Pending,
    /// The listing failed.
    Failed,
    Listed(&'a InputDevices),
}

/// What the first run's capsule shows, and the inputs its menu offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MicPick {
    pub(crate) shown: Shown,
    pub(crate) choices: Vec<MicChoice>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Shown {
    /// The inputs aren't listed yet.
    #[default]
    Pending,
    /// The input Hex records from, by its shown name.
    Device(String),
    /// Nothing to record from.
    NoInput,
    /// The listing failed.
    Failed,
}

impl Shown {
    /// The capsule's text.
    pub(crate) fn text(&self) -> String {
        match self {
            Shown::Pending => tr("System default").to_owned(),
            Shown::Device(name) => name.clone(),
            Shown::NoInput => tr("No microphone found").to_owned(),
            Shown::Failed => tr("Couldn't list microphones").to_owned(),
        }
    }

    /// Whether the capsule opens a menu of inputs.
    pub(crate) fn picks(&self) -> bool {
        matches!(self, Shown::Pending | Shown::Device(_))
    }
}

/// One input in the capsule's menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MicChoice {
    pub(crate) label: String,
    pub(crate) detail: Option<String>,
    /// What choosing saves as `microphone`: `None` is System default.
    pub(crate) id: Option<String>,
    pub(crate) selected: bool,
}

/// The input to show and the menu to offer, from the listing, the saved
/// `microphone` and the user's names for inputs (`aliases`). A saved input
/// that is listed is the one in use; otherwise System default resolves to
/// the listing's `system_default`, which is never Hex Microphone.
pub(crate) fn pick(
    listing: Listing<'_>,
    saved: Option<&str>,
    aliases: &BTreeMap<String, String>,
) -> MicPick {
    let system_default = MicChoice {
        label: tr("System default").to_owned(),
        detail: None,
        id: None,
        selected: saved.is_none(),
    };
    let devices = match listing {
        Listing::Pending => {
            return MicPick {
                shown: Shown::Pending,
                choices: vec![system_default],
            };
        }
        Listing::Failed => {
            return MicPick {
                shown: Shown::Failed,
                choices: Vec::new(),
            };
        }
        Listing::Listed(devices) => devices,
    };
    let label = |id: &str| devices.label(aliases, id);
    let is_listed = |id: &str| {
        devices.names.iter().any(|name| name == id)
            || devices.devices.iter().any(|device| device.id == id)
    };
    let off = devices.off_with_lid_closed.as_deref();
    let default_name = devices
        .system_default
        .as_deref()
        .filter(|id| !crate::audio::hex_microphone::is_hex_virtual_device_name(id))
        .map(label);
    let in_use = saved.filter(|saved| is_listed(saved) && off != Some(*saved));
    let shown = match (in_use, &default_name) {
        (Some(saved), _) => Shown::Device(label(saved)),
        (None, Some(name)) => Shown::Device(name.clone()),
        (None, None) if devices.names.is_empty() => Shown::NoInput,
        (None, None) => Shown::Device(tr("System default").to_owned()),
    };
    let mut choices = vec![MicChoice {
        detail: default_name,
        ..system_default
    }];
    let mut ids: Vec<&str> = devices.names.iter().map(String::as_str).collect();
    // A saved input hidden by default (a monitor, say) stays offered.
    if let Some(saved) = saved.filter(|saved| is_listed(saved) && !ids.contains(saved)) {
        ids.push(saved);
    }
    choices.extend(ids.into_iter().map(|id| {
        let bluetooth = devices
            .devices
            .iter()
            .any(|device| device.id == id && device.kind == DeviceKind::Bluetooth);
        MicChoice {
            label: label(id),
            detail: if off == Some(id) {
                Some(tr("off while the lid is closed").to_owned())
            } else {
                bluetooth.then(|| tr("Bluetooth").to_owned())
            },
            id: Some(id.to_owned()),
            selected: saved == Some(id),
        }
    }));
    if let Some(saved) = saved.filter(|saved| !is_listed(saved)) {
        choices.push(MicChoice {
            label: label(saved),
            detail: Some(tr("not connected").to_owned()),
            id: Some(saved.to_owned()),
            selected: true,
        });
    }
    if shown == Shown::NoInput {
        choices.clear();
    }
    MicPick { shown, choices }
}

/// Standalone microphone line used by the earlier option stories.
pub(crate) fn mic_line(p: Paint, shown: bool, peak: Option<f32>) -> El {
    row().key("mic-line").h(H).center().children(
        shown.then(|| mic_capsule(p, peak, DEMO_NAME, true).transition(Transition::opacity())),
    )
}

/// Tactile `28 pt` microphone capsule naming `name`, with a live 3-bar
/// input meter. A capsule that `picks` is a button with a chevron; one that
/// doesn't only says why there is nothing to pick.
pub(crate) fn mic_capsule(p: Paint, peak: Option<f32>, name: &str, picks: bool) -> El {
    let k = p.k;
    let c = &k.c;
    let lvl = level(peak);
    let mut cap = row()
        .key("mic-capsule")
        .label(format!("{}, {name}", tr("Microphone")))
        .h(H)
        .pl(10.0)
        .pr(if picks { 8.0 } else { 11.0 })
        .gap(7.0)
        .center_y()
        .radius(H * 0.5)
        .fill(c.surface_raised)
        .border(p.hair, c.border_weak)
        .child(bars(p, lvl))
        .child(
            text(name.to_owned())
                .key("name")
                .font(DESCRIPTION)
                .color(p.muted())
                .truncate_fade(),
        );
    if picks {
        cap = cap
            .role("button")
            .focusable(true)
            .focus_ring(1.5, 1.0, c.border_focus)
            .hover(c.hover)
            .press(c.pressed)
            .child(
                icon(k.glyph.chevron_down, 12.0)
                    .key("chevron")
                    .color(c.icon_muted),
            );
    }
    for shadow in k.shadow_layers(&k.elevation.button_neutral) {
        cap = cap.box_shadow(shadow);
    }
    cap
}

fn bars(p: Paint, level: f32) -> El {
    const WEIGHTS: [f32; 3] = [0.6, 1.0, 0.75];
    let mut out = rune::el().key("level").size(12.0, 14.0);
    for (i, w) in WEIGHTS.into_iter().enumerate() {
        let h = 2.0 + w * 3.5 + level * w * 7.5;
        out = out.child(
            rune::el()
                .key(format!("b{i}"))
                .absolute(1.0 + i as f32 * 4.0, (14.0 - h) * 0.5)
                .size(2.0, h)
                .motion(Spring::new(0.12, 0.0))
                .radius(1.0)
                .fill(if level > 0.0 { p.text() } else { p.faint() }),
        );
    }
    out
}

fn level(peak: Option<f32>) -> f32 {
    peak.map_or(0.0, |peak| (peak * 3.0).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::InputDevice;

    #[test]
    fn the_level_rests_until_hex_listens_then_follows_speech() {
        assert_eq!(level(None), 0.0);
        assert!((level(Some(0.1)) - 0.3).abs() < 1e-6);
        assert_eq!(level(Some(0.9)), 1.0);
    }

    fn device(id: &str, label: &str, kind: DeviceKind, hidden: bool) -> InputDevice {
        InputDevice {
            id: id.into(),
            label: label.into(),
            kind,
            hidden,
            music_mode: false,
        }
    }

    /// A Windows laptop: its array microphone (the default) and a USB one.
    fn windows() -> InputDevices {
        let devices = vec![
            device(
                "Microphone Array (Realtek(R) Audio)",
                "Microphone Array (Realtek(R) Audio)",
                DeviceKind::BuiltIn,
                false,
            ),
            device("USB Mic", "USB Mic", DeviceKind::Usb, false),
        ];
        InputDevices {
            names: devices.iter().map(|d| d.id.clone()).collect(),
            devices,
            system_default: Some("Microphone Array (Realtek(R) Audio)".into()),
            ..Default::default()
        }
    }

    fn selected(pick: &MicPick) -> Vec<Option<&str>> {
        pick.choices
            .iter()
            .filter(|choice| choice.selected)
            .map(|choice| choice.id.as_deref())
            .collect()
    }

    #[test]
    fn nothing_saved_names_the_system_default_and_selects_it() {
        let devices = windows();
        let pick = pick(Listing::Listed(&devices), None, &BTreeMap::new());
        assert_eq!(
            pick.shown,
            Shown::Device("Microphone Array (Realtek(R) Audio)".into())
        );
        assert_eq!(selected(&pick), vec![None]);
        assert_eq!(
            pick.choices[0].detail.as_deref(),
            Some("Microphone Array (Realtek(R) Audio)")
        );
        let ids: Vec<_> = pick.choices.iter().map(|c| c.id.as_deref()).collect();
        assert_eq!(
            ids,
            vec![
                None,
                Some("Microphone Array (Realtek(R) Audio)"),
                Some("USB Mic")
            ]
        );
        assert!(pick.shown.picks());
    }

    #[test]
    fn a_saved_microphone_is_shown_and_selected() {
        let devices = windows();
        let pick = pick(Listing::Listed(&devices), Some("USB Mic"), &BTreeMap::new());
        assert_eq!(pick.shown, Shown::Device("USB Mic".into()));
        assert_eq!(selected(&pick), vec![Some("USB Mic")]);
    }

    #[test]
    fn the_users_own_name_for_an_input_is_shown() {
        let devices = windows();
        let aliases = BTreeMap::from([("USB Mic".to_owned(), "Desk mic".to_owned())]);
        let pick = pick(Listing::Listed(&devices), Some("USB Mic"), &aliases);
        assert_eq!(pick.shown, Shown::Device("Desk mic".into()));
    }

    #[test]
    fn a_saved_microphone_that_is_gone_shows_the_default_in_its_place() {
        let devices = windows();
        let pick = pick(
            Listing::Listed(&devices),
            Some("Studio Mic"),
            &BTreeMap::new(),
        );
        assert_eq!(
            pick.shown,
            Shown::Device("Microphone Array (Realtek(R) Audio)".into())
        );
        let gone = pick.choices.last().unwrap();
        assert_eq!(gone.id.as_deref(), Some("Studio Mic"));
        assert_eq!(gone.detail.as_deref(), Some("not connected"));
        assert_eq!(selected(&pick), vec![Some("Studio Mic")]);
    }

    #[test]
    fn hex_microphone_is_never_named_as_the_input() {
        // The listing names the stand-in when Hex Microphone is the OS
        // default; a stray Hex default is still never shown.
        let mut devices = windows();
        devices.system_default = Some("Hex Microphone".into());
        devices.default_is_hex_microphone = true;
        let pick = pick(Listing::Listed(&devices), None, &BTreeMap::new());
        assert_eq!(pick.shown, Shown::Device("System default".into()));
        assert_eq!(pick.choices[0].detail, None);

        devices.system_default = Some("USB Mic".into());
        let pick = super::pick(Listing::Listed(&devices), None, &BTreeMap::new());
        assert_eq!(pick.shown, Shown::Device("USB Mic".into()));
    }

    #[test]
    fn no_input_says_so_and_offers_nothing() {
        let none = InputDevices::default();
        let pick = pick(Listing::Listed(&none), None, &BTreeMap::new());
        assert_eq!(pick.shown, Shown::NoInput);
        assert_eq!(pick.shown.text(), "No microphone found");
        assert!(!pick.shown.picks());
        assert!(pick.choices.is_empty());

        let saved = super::pick(Listing::Listed(&none), Some("USB Mic"), &BTreeMap::new());
        assert_eq!(saved.shown, Shown::NoInput);
    }

    #[test]
    fn inputs_without_a_known_default_say_system_default() {
        // Linux without a sound server lists inputs but no default.
        let mut devices = windows();
        devices.system_default = None;
        let pick = pick(Listing::Listed(&devices), None, &BTreeMap::new());
        assert_eq!(pick.shown, Shown::Device("System default".into()));
        assert_eq!(selected(&pick), vec![None]);
    }

    #[test]
    fn a_failed_or_pending_listing_never_names_a_device() {
        let failed = pick(Listing::Failed, None, &BTreeMap::new());
        assert_eq!(failed.shown, Shown::Failed);
        assert!(!failed.shown.picks());
        let pending = pick(Listing::Pending, Some("USB Mic"), &BTreeMap::new());
        assert_eq!(pending.shown.text(), "System default");
        assert!(pending.shown.picks());
    }

    #[test]
    fn the_lid_closed_built_in_stands_aside_for_the_default() {
        let mut devices = windows();
        devices.off_with_lid_closed = Some("Microphone Array (Realtek(R) Audio)".into());
        devices.system_default = Some("USB Mic".into());
        let pick = pick(
            Listing::Listed(&devices),
            Some("Microphone Array (Realtek(R) Audio)"),
            &BTreeMap::new(),
        );
        assert_eq!(pick.shown, Shown::Device("USB Mic".into()));
        assert_eq!(
            pick.choices[1].detail.as_deref(),
            Some("off while the lid is closed")
        );
    }

    #[test]
    fn a_saved_hidden_input_stays_offered_and_bluetooth_is_marked() {
        let mut devices = windows();
        devices.devices.push(device(
            "monitor",
            "Monitor of Speakers",
            DeviceKind::Virtual,
            true,
        ));
        devices.devices.push(device(
            "bluez_input.buds",
            "Buds",
            DeviceKind::Bluetooth,
            false,
        ));
        devices.names.push("bluez_input.buds".into());
        let pick = pick(Listing::Listed(&devices), Some("monitor"), &BTreeMap::new());
        assert_eq!(pick.shown, Shown::Device("Monitor of Speakers".into()));
        assert_eq!(selected(&pick), vec![Some("monitor")]);
        let buds = pick
            .choices
            .iter()
            .find(|c| c.id.as_deref() == Some("bluez_input.buds"))
            .unwrap();
        assert_eq!(buds.detail.as_deref(), Some("Bluetooth"));
    }
}
