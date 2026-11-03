//! `wordink devices`: the microphones WordInk offers, with the same clean
//! names Settings shows, and `wordink devices rename` to give one a name.

use std::collections::BTreeMap;

use clap::Subcommand;

use crate::audio::{InputDevices, sources};

#[derive(Subcommand, Debug)]
pub enum DevicesCommand {
    /// Give a microphone your own name, by id or its current name. An empty
    /// name goes back to the default one.
    Rename { device: String, name: String },
}

/// One line per microphone: its name (yours, if you renamed it), how it
/// connects, and the id `microphone` saves. Monitors, null devices and
/// effect nodes only with `--all`.
pub fn format_devices(
    devices: &InputDevices,
    aliases: &BTreeMap<String, String>,
    all: bool,
) -> Vec<String> {
    devices
        .devices
        .iter()
        .filter(|device| all || !device.hidden)
        .map(|device| {
            let default = devices.system_default.as_deref() == Some(device.id.as_str());
            format!(
                "{}\t{}\t{}{}",
                sources::label_for(aliases, &device.id, Some(&device.label)),
                kind_name(device.kind),
                device.id,
                if default { "\tdefault" } else { "" }
            )
        })
        .collect()
}

fn kind_name(kind: sources::DeviceKind) -> &'static str {
    match kind {
        sources::DeviceKind::BuiltIn => "built-in",
        sources::DeviceKind::Usb => "usb",
        sources::DeviceKind::Bluetooth => "bluetooth",
        sources::DeviceKind::Virtual => "virtual",
        sources::DeviceKind::Other => "other",
    }
}

/// The id `query` names: an id exactly, else a name (yours or the default
/// one) ignoring case.
pub fn resolve_device(
    devices: &InputDevices,
    aliases: &BTreeMap<String, String>,
    query: &str,
) -> Result<String, String> {
    if devices.devices.iter().any(|device| device.id == query) {
        return Ok(query.to_owned());
    }
    let matches: Vec<&str> = devices
        .devices
        .iter()
        .filter(|device| {
            sources::label_for(aliases, &device.id, Some(&device.label)).eq_ignore_ascii_case(query)
                || device.label.eq_ignore_ascii_case(query)
        })
        .map(|device| device.id.as_str())
        .collect();
    match matches.as_slice() {
        [id] => Ok((*id).to_owned()),
        [] => Err(format!(
            "no microphone is called `{query}`; run `wordink devices --all` for the ids"
        )),
        _ => Err(format!(
            "more than one microphone is called `{query}`; use its id from `wordink devices`"
        )),
    }
}

/// `aliases` with `id` renamed; a blank name removes it.
pub fn renamed(
    aliases: &BTreeMap<String, String>,
    id: &str,
    name: &str,
) -> BTreeMap<String, String> {
    let mut next = aliases.clone();
    match name.trim() {
        "" => next.remove(id),
        name => next.insert(id.to_owned(), name.to_owned()),
    };
    next
}

pub fn run(all: bool, command: Option<DevicesCommand>) -> i32 {
    match devices(all, command) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn devices(all: bool, command: Option<DevicesCommand>) -> Result<(), String> {
    let listed = crate::audio::input_devices().map_err(|error| format!("{error:#}"))?;
    let settings = crate::app_settings::AppSettings::read().unwrap_or_default();
    match command {
        None => {
            for line in format_devices(&listed, &settings.device_names, all) {
                println!("{line}");
            }
            Ok(())
        }
        Some(DevicesCommand::Rename { device, name }) => {
            let id = resolve_device(&listed, &settings.device_names, &device)?;
            let value = serde_json::to_string(&renamed(&settings.device_names, &id, &name))
                .map_err(|error| error.to_string())?;
            let reply = match crate::control::cli::set_in_running_hex("device_names", &value) {
                Some(reply) => reply,
                None => crate::control::cli::set_offline("device_names", &value),
            };
            match reply {
                crate::control::ControlReply::Error { message, .. } => Err(message),
                _ => {
                    println!(
                        "{id}\t{}",
                        if name.trim().is_empty() {
                            "default name"
                        } else {
                            name.trim()
                        }
                    );
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::InputDevice;
    use sources::DeviceKind;

    fn device(id: &str, label: &str, kind: DeviceKind, hidden: bool) -> InputDevice {
        InputDevice {
            id: id.into(),
            label: label.into(),
            kind,
            hidden,
            music_mode: false,
        }
    }

    fn listing() -> InputDevices {
        InputDevices {
            names: vec!["mic-a".into(), "bluez_input.00:11:22:33:44:55".into()],
            devices: vec![
                device("mic-a", "Studio Mic", DeviceKind::Usb, false),
                device(
                    "bluez_input.00:11:22:33:44:55",
                    "Test Buds",
                    DeviceKind::Bluetooth,
                    false,
                ),
                device("easyeffects_source", "Effects", DeviceKind::Virtual, true),
            ],
            system_default: Some("mic-a".into()),
            ..Default::default()
        }
    }

    #[test]
    fn the_list_shows_clean_names_and_hides_effect_nodes_until_asked() {
        let aliases = BTreeMap::from([("mic-a".to_owned(), "Desk mic".to_owned())]);
        assert_eq!(
            format_devices(&listing(), &aliases, false),
            [
                "Desk mic\tusb\tmic-a\tdefault",
                "Test Buds\tbluetooth\tbluez_input.00:11:22:33:44:55"
            ]
        );
        assert_eq!(format_devices(&listing(), &aliases, true).len(), 3);
    }

    #[test]
    fn a_device_is_found_by_id_or_by_either_name() {
        let aliases = BTreeMap::from([("mic-a".to_owned(), "Desk mic".to_owned())]);
        let listed = listing();
        assert_eq!(resolve_device(&listed, &aliases, "mic-a").unwrap(), "mic-a");
        assert_eq!(
            resolve_device(&listed, &aliases, "desk MIC").unwrap(),
            "mic-a"
        );
        assert_eq!(
            resolve_device(&listed, &aliases, "studio mic").unwrap(),
            "mic-a"
        );
        assert!(resolve_device(&listed, &aliases, "nothing").is_err());
    }

    #[test]
    fn renaming_sets_replaces_and_clears_a_name() {
        let none = BTreeMap::new();
        let named = renamed(&none, "mic-a", "  Desk mic ");
        assert_eq!(named["mic-a"], "Desk mic");
        assert_eq!(renamed(&named, "mic-a", "Podcast")["mic-a"], "Podcast");
        assert!(renamed(&named, "mic-a", "  ").is_empty());
    }
}
