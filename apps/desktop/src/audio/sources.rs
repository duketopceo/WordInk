//! The microphones Settings offers, as the sound server describes them, and
//! the names they show. Everything here is pure: Linux feeds it the JSON of
//! `pactl -f json list sources` and `list cards`, and the other platforms
//! use only [`clean_name`] and [`label_for`] on the names their audio API
//! reports.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

/// How a microphone connects.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DeviceKind {
    BuiltIn,
    Usb,
    Bluetooth,
    Virtual,
    Other,
}

/// A Bluetooth headset's card, which has a microphone only in headset mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BluetoothCard {
    /// The card's name, such as `bluez_card.AA_BB_CC_DD_EE_FF`.
    pub card: String,
    /// The card's active profile, such as `a2dp-sink` (music mode) or
    /// `headset-head-unit` (headset mode).
    pub profile: Option<String>,
    /// The node name of its microphone while it is in headset mode.
    pub source: Option<String>,
}

/// One microphone as Settings lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    /// Stable id: the PipeWire node name. A Bluetooth headset's id is
    /// `bluez_input.<address>` whether or not it is in headset mode.
    pub id: String,
    /// The cleaned-up name, before the user's own name for it.
    pub label: String,
    pub kind: DeviceKind,
    pub bluetooth: Option<BluetoothCard>,
    /// Hidden from the picker unless it shows every device: a monitor, null
    /// device, or effect or virtual node.
    pub hidden: bool,
}

impl Device {
    /// A Bluetooth headset sitting in music mode, with no microphone yet.
    pub fn needs_headset_mode(&self) -> bool {
        self.bluetooth.as_ref().is_some_and(|card| {
            card.source.is_none() && card.profile.as_deref().is_some_and(is_music_profile)
        })
    }
}

/// An A2DP (music) profile: high quality, no microphone.
pub fn is_music_profile(profile: &str) -> bool {
    profile.starts_with("a2dp")
}

#[derive(Debug, Default, Deserialize)]
pub struct PulseSource {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
}

impl PulseSource {
    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).and_then(|value| value.as_str())
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct PulseProfile {
    #[serde(default = "yes")]
    pub available: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
pub struct PulseCard {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub profiles: HashMap<String, PulseProfile>,
}

impl PulseCard {
    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).and_then(|value| value.as_str())
    }

    fn is_bluetooth(&self) -> bool {
        self.property("device.api") == Some("bluez5") || self.name.starts_with("bluez_card.")
    }

    /// The headset profile to switch to: mSBC (wideband) when offered, else CVSD.
    pub fn headset_profile(&self) -> Option<&'static str> {
        ["headset-head-unit", "headset-head-unit-cvsd"]
            .into_iter()
            .find(|name| self.profiles.get(*name).is_some_and(|p| p.available))
    }
}

pub fn parse_sources(json: &str) -> Result<Vec<PulseSource>, String> {
    serde_json::from_str(json).map_err(|error| format!("could not read the sources: {error}"))
}

pub fn parse_cards(json: &str) -> Result<Vec<PulseCard>, String> {
    serde_json::from_str(json).map_err(|error| format!("could not read the cards: {error}"))
}

/// A Bluetooth address (`AA:BB:CC:DD:EE:FF`, also written with `_` or `-`)
/// found in `text`, normalised to colons and upper case.
pub fn find_address(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len().saturating_sub(16)).find_map(|start| {
        let window = &chars[start..start + 17];
        let shaped = window.chunks(3).all(|chunk| {
            chunk[..2].iter().all(char::is_ascii_hexdigit)
                && chunk
                    .get(2)
                    .is_none_or(|separator| matches!(separator, ':' | '_' | '-'))
        });
        let alone = (start == 0 || !chars[start - 1].is_ascii_hexdigit())
            && chars.get(start + 17).is_none_or(|c| !c.is_ascii_hexdigit());
        (shaped && alone).then(|| {
            window
                .chunks(3)
                .map(|chunk| chunk[..2].iter().collect::<String>().to_ascii_uppercase())
                .collect::<Vec<_>>()
                .join(":")
        })
    })
}

/// The id of a Bluetooth headset's microphone from its address.
pub fn bluetooth_id(address: &str) -> String {
    format!("bluez_input.{address}")
}

fn address_of_card(card: &PulseCard) -> Option<String> {
    card.property("api.bluez5.address")
        .and_then(find_address)
        .or_else(|| find_address(&card.name))
}

fn address_of_source(source: &PulseSource) -> Option<String> {
    source
        .property("api.bluez5.address")
        .and_then(find_address)
        .or_else(|| find_address(&source.name))
}

fn is_bluetooth_source(source: &PulseSource) -> bool {
    source.property("device.api") == Some("bluez5") || source.name.starts_with("bluez_")
}

/// Why the picker leaves a source out unless it shows every device.
fn is_hidden(source: &PulseSource) -> bool {
    let name = source.name.to_ascii_lowercase();
    let description = source.description.to_ascii_lowercase();
    source.property("device.class") == Some("monitor")
        || name.ends_with(".monitor")
        || source
            .property("media.class")
            .is_some_and(|class| !class.starts_with("Audio/Source") || class.ends_with("/Virtual"))
        || source.property("node.virtual") == Some("true")
        || [
            "effect_",
            "audio_effect.",
            "easyeffects",
            "auto_null",
            "null",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || ["dummy output", "discard all", "null output"]
            .iter()
            .any(|word| description.contains(word))
}

fn kind_of(source: &PulseSource, hidden: bool) -> DeviceKind {
    let property = |key| source.property(key);
    if is_bluetooth_source(source) {
        DeviceKind::Bluetooth
    } else if hidden {
        DeviceKind::Virtual
    } else if property("device.bus") == Some("usb") {
        DeviceKind::Usb
    } else if property("device.form_factor") == Some("internal")
        || (property("device.api") == Some("alsa")
            && matches!(
                property("device.bus"),
                None | Some("pci") | Some("platform")
            )
            && property("device.form_factor") != Some("webcam"))
    {
        DeviceKind::BuiltIn
    } else {
        DeviceKind::Other
    }
}

/// Every microphone the sound server has, hidden ones flagged: one device
/// per source, plus a Bluetooth headset in music mode, which has a card but
/// no source until it switches to headset mode. Built-in first, then USB,
/// Bluetooth, other; equal labels get a number.
pub fn devices(sources: &[PulseSource], cards: &[PulseCard]) -> Vec<Device> {
    let mut found: Vec<Device> = Vec::new();
    let mut headsets_seen: Vec<String> = Vec::new();
    for source in sources {
        let hidden = is_hidden(source);
        let kind = kind_of(source, hidden);
        if kind == DeviceKind::Bluetooth {
            // A headset's own microphone; its monitors are hidden anyway.
            let Some(address) = address_of_source(source) else {
                continue;
            };
            if hidden || !source.name.starts_with("bluez_input.") {
                continue;
            }
            let card = cards
                .iter()
                .find(|card| address_of_card(card).as_deref() == Some(address.as_str()));
            let alias = source
                .property("device.alias")
                .or_else(|| card.and_then(|card| card.property("device.alias")))
                .unwrap_or(&source.description);
            headsets_seen.push(address.clone());
            found.push(Device {
                id: bluetooth_id(&address),
                label: clean_name_for(DeviceKind::Bluetooth, alias),
                kind,
                bluetooth: Some(BluetoothCard {
                    card: card.map_or_else(
                        || format!("bluez_card.{}", address.replace(':', "_")),
                        |card| card.name.clone(),
                    ),
                    profile: card.and_then(|card| card.active_profile.clone()),
                    source: Some(source.name.clone()),
                }),
                hidden: false,
            });
            continue;
        }
        found.push(Device {
            id: source.name.clone(),
            label: clean_name_for(kind, &source.description),
            kind,
            bluetooth: None,
            hidden,
        });
    }
    for card in cards.iter().filter(|card| card.is_bluetooth()) {
        let Some(address) = address_of_card(card) else {
            continue;
        };
        if headsets_seen.contains(&address) || card.headset_profile().is_none() {
            continue;
        }
        let alias = card
            .property("device.alias")
            .or_else(|| card.property("device.description"))
            .unwrap_or("");
        found.push(Device {
            id: bluetooth_id(&address),
            label: clean_name_for(DeviceKind::Bluetooth, alias),
            kind: DeviceKind::Bluetooth,
            bluetooth: Some(BluetoothCard {
                card: card.name.clone(),
                profile: card.active_profile.clone(),
                source: None,
            }),
            hidden: false,
        });
    }
    found.sort_by(|a, b| {
        (a.kind, a.label.to_lowercase(), &a.id).cmp(&(b.kind, b.label.to_lowercase(), &b.id))
    });
    number_duplicates(&mut found);
    found
}

/// Two devices called the same get " 2", " 3", … so they can be told apart.
fn number_duplicates(devices: &mut [Device]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut totals: HashMap<String, usize> = HashMap::new();
    for device in devices.iter() {
        *totals.entry(device.label.clone()).or_insert(0) += 1;
    }
    for device in devices {
        if totals[&device.label] > 1 {
            let count = seen.entry(device.label.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                device.label = format!("{} {}", device.label, count);
            }
        }
    }
}

/// The id a source name stands for: a Bluetooth microphone's name collapses
/// to its `bluez_input.<address>` id; anything else is its own id.
pub fn id_of_source_name(name: &str) -> String {
    match name
        .starts_with("bluez_input.")
        .then(|| find_address(name))
        .flatten()
    {
        Some(address) => bluetooth_id(&address),
        None => name.to_owned(),
    }
}

/// The name shown for `id`: the user's own, else the listed label, else the id.
pub fn label_for(aliases: &BTreeMap<String, String>, id: &str, listed: Option<&str>) -> String {
    aliases
        .get(id)
        .map(|alias| alias.trim())
        .filter(|alias| !alias.is_empty())
        .or(listed)
        .unwrap_or(id)
        .to_owned()
}

/// Parts of an ALSA name after a comma that name a driver, not a device.
fn is_driver_segment(segment: &str) -> bool {
    let lower = segment.trim().to_ascii_lowercase();
    lower.starts_with("usb audio")
        || lower.starts_with("usb-audio")
        || lower == "generic"
        || lower.starts_with("generic ")
        || (!lower.contains(' ') && lower.contains('_'))
}

fn is_address(token: &str) -> bool {
    let inner = token.trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'));
    inner.len() == 17 && find_address(inner).is_some()
}

/// Trailing words that name a sound profile, not the device.
const PROFILE_WORDS: &[&[&str]] = &[
    &["analog", "stereo"],
    &["analog", "mono"],
    &["digital", "stereo"],
    &["multichannel"],
    &["stereo"],
    &["mono"],
];

fn ends_with_words(words: &[&str], tail: &[&str]) -> bool {
    words.len() >= tail.len()
        && words[words.len() - tail.len()..]
            .iter()
            .zip(tail)
            .all(|(word, want)| word.eq_ignore_ascii_case(want))
}

/// Trims the garble sound servers add to a device name: ALSA's driver
/// suffix (", USB Audio"), a profile ("Analog Stereo"), raw Bluetooth
/// addresses and a vendor said twice. A name that cleans to nothing is
/// "Microphone".
pub fn clean_name(raw: &str) -> String {
    clean_name_for(DeviceKind::Other, raw)
}

pub fn clean_name_for(kind: DeviceKind, raw: &str) -> String {
    let mut segments = raw.split(',');
    let mut text = segments.next().unwrap_or("").to_owned();
    for segment in segments.filter(|segment| !is_driver_segment(segment)) {
        text.push(' ');
        text.push_str(segment);
    }
    let mut words: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !is_address(word))
        .collect();
    // Profiles come off the end, after an optional "(IEC958)"-style tag.
    loop {
        let before = words.len();
        if words.len() > 2
            && words
                .last()
                .is_some_and(|word| word.starts_with('(') && word.ends_with(')'))
            && PROFILE_WORDS
                .iter()
                .any(|profile| ends_with_words(&words[..words.len() - 1], profile))
        {
            words.pop();
        }
        if let Some(profile) = PROFILE_WORDS
            .iter()
            .find(|profile| words.len() > profile.len() && ends_with_words(&words, profile))
        {
            words.truncate(words.len() - profile.len());
        }
        if words.len() == before {
            break;
        }
    }
    let mut cleaned: Vec<&str> = Vec::with_capacity(words.len());
    for word in words {
        if cleaned
            .last()
            .is_none_or(|last| !last.eq_ignore_ascii_case(word))
        {
            cleaned.push(word);
        }
    }
    let cleaned = cleaned.join(" ");
    if !cleaned.is_empty() {
        cleaned
    } else if kind == DeviceKind::Bluetooth {
        "Bluetooth headset".into()
    } else {
        "Microphone".into()
    }
}

/// Names the ALSA fallback never offers: PCM plumbing, not microphones.
pub fn is_alsa_plumbing(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("discard all samples") || lower.starts_with("default alsa")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCES: &str = include_str!("sources_fixtures/sources_a2dp.json");
    const CARDS_A2DP: &str = include_str!("sources_fixtures/cards_a2dp.json");
    const SOURCES_HEADSET: &str = include_str!("sources_fixtures/sources_headset.json");
    const CARDS_HEADSET: &str = include_str!("sources_fixtures/cards_headset.json");

    fn list(sources: &str, cards: &str) -> Vec<Device> {
        devices(
            &parse_sources(sources).unwrap(),
            &parse_cards(cards).unwrap(),
        )
    }

    fn visible(devices: &[Device]) -> Vec<(&str, &str, DeviceKind)> {
        devices
            .iter()
            .filter(|device| !device.hidden)
            .map(|device| (device.id.as_str(), device.label.as_str(), device.kind))
            .collect()
    }

    #[test]
    fn names_are_cleaned_of_sound_server_garble() {
        let table = [
            ("NexiGo N60 FHD Webcam, USB Audio", "NexiGo N60 FHD Webcam"),
            ("NexiGo N60 FHD Webcam  Mono", "NexiGo N60 FHD Webcam"),
            ("Acme Microphone Analog Stereo", "Acme Microphone"),
            ("Laptop Mic HPAI, aop_audio", "Laptop Mic HPAI"),
            (
                "Logitech Logitech BRIO Digital Stereo (IEC958)",
                "Logitech BRIO",
            ),
            ("Built-in Audio Analog Stereo", "Built-in Audio"),
            ("Blue Microphones Mono", "Blue Microphones"),
            ("USB PnP Sound Device, USB Audio #2", "USB PnP Sound Device"),
            ("Test Buds (AA:BB:CC:DD:EE:FF)", "Test Buds"),
            ("Razer Razer Seiren Mini", "Razer Seiren Mini"),
            ("Stereo", "Stereo"),
            ("   ", "Microphone"),
            ("aa-bb-cc-dd-ee-ff", "Microphone"),
            ("Laptop Mic (raw)", "Laptop Mic (raw)"),
        ];
        for (raw, want) in table {
            assert_eq!(clean_name(raw), want, "cleaning {raw:?}");
        }
        for raw in ["AA:BB:CC:DD:EE:FF", "AA_BB_CC_DD_EE_FF"] {
            assert_eq!(
                clean_name_for(DeviceKind::Bluetooth, raw),
                "Bluetooth headset"
            );
        }
    }

    #[test]
    fn addresses_are_found_in_any_spelling() {
        assert_eq!(
            find_address("bluez_card.aa_bb_cc_dd_ee_ff").as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
        assert_eq!(
            find_address("bluez_input.AA:BB:CC:DD:EE:FF.0").as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
        assert_eq!(find_address("alsa_input.usb-Acme_Mic-00"), None);
        assert_eq!(
            id_of_source_name("bluez_input.AA_BB_CC_DD_EE_FF.0"),
            "bluez_input.AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(id_of_source_name("alsa_input.pci-0"), "alsa_input.pci-0");
    }

    #[test]
    fn an_alias_beats_the_listed_name_and_blank_aliases_are_ignored() {
        let mut aliases = BTreeMap::new();
        aliases.insert("mic".to_owned(), "  Desk mic ".to_owned());
        aliases.insert("blank".to_owned(), "   ".to_owned());
        assert_eq!(
            label_for(&aliases, "mic", Some("Acme Microphone")),
            "Desk mic"
        );
        assert_eq!(label_for(&aliases, "blank", Some("Acme")), "Acme");
        assert_eq!(label_for(&aliases, "other", Some("Acme")), "Acme");
        assert_eq!(label_for(&aliases, "other", None), "other");
    }

    #[test]
    fn the_list_hides_monitors_null_and_effect_nodes() {
        let devices = list(SOURCES, CARDS_A2DP);
        assert_eq!(
            visible(&devices),
            vec![
                (
                    "alsa_input.platform-sound.HiFi__Headset__source",
                    "Laptop Mic (raw)",
                    DeviceKind::BuiltIn
                ),
                (
                    "alsa_input.usb-acme_Acme_N60_FHD_Webcam_Webcam-02.mono-fallback",
                    "Acme N60 FHD Webcam",
                    DeviceKind::Usb
                ),
                (
                    "alsa_input.usb-acme_Studio_Mic-00.analog-stereo",
                    "Acme Studio Mic",
                    DeviceKind::Usb
                ),
                (
                    "bluez_input.00:11:22:33:44:55",
                    "Test Buds",
                    DeviceKind::Bluetooth
                ),
            ]
        );
        let hidden: Vec<_> = devices
            .iter()
            .filter(|device| device.hidden)
            .map(|device| device.id.as_str())
            .collect();
        for expected in [
            "easyeffects_sink.monitor",
            "easyeffects_source",
            "effect_output.laptop-mic",
            "auto_null.monitor",
        ] {
            assert!(
                hidden.contains(&expected),
                "{expected} hidden in {hidden:?}"
            );
        }
    }

    #[test]
    fn earbuds_in_music_mode_are_listed_from_their_card() {
        let devices = list(SOURCES, CARDS_A2DP);
        let buds = devices
            .iter()
            .find(|device| device.kind == DeviceKind::Bluetooth)
            .unwrap();
        let card = buds.bluetooth.as_ref().unwrap();
        assert_eq!(card.card, "bluez_card.00_11_22_33_44_55");
        assert_eq!(card.profile.as_deref(), Some("a2dp-sink"));
        assert_eq!(card.source, None);
        assert!(buds.needs_headset_mode());
    }

    #[test]
    fn earbuds_in_headset_mode_are_listed_from_their_source_under_the_same_id() {
        let find = |devices: Vec<Device>| {
            devices
                .into_iter()
                .find(|device| device.kind == DeviceKind::Bluetooth)
                .unwrap()
        };
        let music = find(list(SOURCES, CARDS_A2DP));
        let headset = find(list(SOURCES_HEADSET, CARDS_HEADSET));
        assert_eq!(music.id, headset.id, "one stable id in both modes");
        assert_eq!(headset.label, "Test Buds");
        let card = headset.bluetooth.as_ref().unwrap();
        assert_eq!(card.profile.as_deref(), Some("headset-head-unit"));
        assert_eq!(
            card.source.as_deref(),
            Some("bluez_input.00_11_22_33_44_55.0")
        );
        assert!(!headset.needs_headset_mode());
    }

    #[test]
    fn a_headset_card_without_a_headset_profile_has_no_microphone() {
        let cards = r#"[{"name":"bluez_card.00_11_22_33_44_55","active_profile":"a2dp-sink",
            "properties":{"device.api":"bluez5","api.bluez5.address":"00:11:22:33:44:55","device.alias":"Speaker"},
            "profiles":{"a2dp-sink":{"available":true},"headset-head-unit":{"available":false}}}]"#;
        assert!(list("[]", cards).is_empty());
    }

    #[test]
    fn headset_profile_prefers_msbc_then_cvsd() {
        let cards = parse_cards(CARDS_A2DP).unwrap();
        let card = cards.iter().find(|card| card.is_bluetooth()).unwrap();
        assert_eq!(card.headset_profile(), Some("headset-head-unit"));
        let cvsd_only = parse_cards(
            r#"[{"name":"bluez_card.00_11_22_33_44_55","profiles":{
                "headset-head-unit":{"available":false},
                "headset-head-unit-cvsd":{"available":true}}}]"#,
        )
        .unwrap();
        assert_eq!(
            cvsd_only[0].headset_profile(),
            Some("headset-head-unit-cvsd")
        );
    }

    #[test]
    fn identical_names_are_numbered() {
        let source = |name: &str| {
            format!(
                r#"{{"name":"{name}","description":"Acme Mic Analog Stereo","properties":{{
                "device.api":"alsa","device.bus":"usb","media.class":"Audio/Source"}}}}"#
            )
        };
        let json = format!(
            "[{},{}]",
            source("alsa_input.usb-a"),
            source("alsa_input.usb-b")
        );
        let labels: Vec<_> = list(&json, "[]")
            .into_iter()
            .map(|device| device.label)
            .collect();
        assert_eq!(labels, ["Acme Mic", "Acme Mic 2"]);
    }

    #[test]
    fn alsa_plumbing_is_not_a_microphone() {
        assert!(is_alsa_plumbing(
            "Discard all samples (playback) or generate zero samples (capture)"
        ));
        assert!(is_alsa_plumbing(
            "Default ALSA Output (currently PipeWire Media Server)"
        ));
        assert!(!is_alsa_plumbing("PipeWire Sound Server"));
    }
}
