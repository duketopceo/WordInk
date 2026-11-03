//! Linux microphones from the sound server (PipeWire through its Pulse
//! protocol, or PulseAudio), read with `pactl`. Hex captures through ALSA's
//! `default` device, which the sound server routes to its default source;
//! to record from another source it moves its own capture stream there by
//! node name once the stream exists. Without `pactl` the caller falls back
//! to ALSA's own list.

use std::process::Command;
use std::time::{Duration, Instant};

use crate::audio::sources::{self, Device};

/// How long a new capture stream has to appear in the sound server.
const STREAM_DEADLINE: Duration = Duration::from_secs(2);
const STREAM_POLL: Duration = Duration::from_millis(40);
const PID_PROPERTY: &str = "hex.process.id";

/// The sound server's microphones and which one it records from by default.
pub(crate) struct Listing {
    pub(crate) devices: Vec<Device>,
    /// The id of the default source, when it is one of `devices`.
    pub(crate) default: Option<String>,
}

pub(crate) fn pactl(args: &[&str]) -> Option<String> {
    let output = Command::new("pactl").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `None` without a sound server to ask (no `pactl`, or it fails).
pub(crate) fn listing() -> Option<Listing> {
    let sources = sources::parse_sources(&pactl(&["-f", "json", "list", "sources"])?)
        .inspect_err(|error| tracing::debug!(%error, "could not read the sound server's sources"))
        .ok()?;
    // Cards only add Bluetooth earbuds in music mode; a failure leaves them out.
    let cards = pactl(&["-f", "json", "list", "cards"])
        .and_then(|json| sources::parse_cards(&json).ok())
        .unwrap_or_default();
    let devices = sources::devices(&sources, &cards);
    let default = pactl(&["get-default-source"])
        .map(|name| sources::id_of_source_name(name.trim()))
        .filter(|id| {
            devices
                .iter()
                .any(|device| device.id == *id && !device.id.ends_with(".monitor"))
        });
    Some(Listing { devices, default })
}

/// Moves this process's capture streams to the source `node` (a PipeWire
/// node name), in the background: a just-opened stream takes a moment to
/// appear, and capture never waits on the sound server.
pub(crate) fn route_streams_to(node: String) {
    let spawned = std::thread::Builder::new()
        .name("input-route".into())
        .spawn(move || {
            let started = Instant::now();
            while started.elapsed() < STREAM_DEADLINE {
                let moved = own_streams()
                    .into_iter()
                    .filter(|stream| {
                        pactl(&["move-source-output", &stream.to_string(), &node]).is_some()
                    })
                    .count();
                if moved > 0 {
                    tracing::info!(source = %node, "recording from the chosen microphone");
                    return;
                }
                std::thread::sleep(STREAM_POLL);
            }
            tracing::warn!(source = %node, "no capture stream appeared to route to the chosen microphone");
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start routing the microphone");
    }
}

fn own_streams() -> Vec<u32> {
    #[derive(serde::Deserialize)]
    struct Output {
        index: u32,
        #[serde(default)]
        properties: std::collections::HashMap<String, serde_json::Value>,
    }
    let pid = std::process::id().to_string();
    pactl(&["-f", "json", "list", "source-outputs"])
        .and_then(|json| serde_json::from_str::<Vec<Output>>(&json).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|output| {
            output
                .properties
                .get(PID_PROPERTY)
                .and_then(|value| value.as_str())
                == Some(pid.as_str())
        })
        .map(|output| output.index)
        .collect()
}
