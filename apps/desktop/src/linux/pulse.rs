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

/// One of this process's capture streams, as the sound server lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OwnStream {
    pub(crate) index: u32,
    /// The index of the source it records from.
    pub(crate) source: u32,
}

/// What routing needs from the sound server. The real one is `pactl`; tests
/// fake it, so none of them touches real audio.
pub(crate) trait SoundServer {
    /// This process's capture streams.
    fn own_streams(&mut self) -> Vec<OwnStream>;
    /// The index of the source named `node`, if the server has it.
    fn source_index(&mut self, node: &str) -> Option<u32>;
    fn move_stream(&mut self, stream: u32, node: &str) -> bool;
    fn pause(&mut self, how_long: Duration);
}

/// How a routing wait went, for the debug log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Routed {
    /// Streams that had to be moved (0 when they were already there).
    pub(crate) moved: usize,
    /// How many times the server was asked.
    pub(crate) polls: u32,
    pub(crate) waited: Duration,
}

/// Moves this process's capture streams to the source `node` and returns
/// only once the server lists every one of them on it. A just-opened stream
/// takes a moment to appear, so this polls up to `deadline`; the error says
/// which step never happened.
pub(crate) fn route_until_confirmed(
    server: &mut impl SoundServer,
    node: &str,
    deadline: Duration,
    poll: Duration,
) -> Result<Routed, String> {
    let mut waited = Duration::ZERO;
    let mut polls = 0;
    let mut moved = 0;
    let mut why = "no capture stream appeared in the sound server".to_owned();
    loop {
        polls += 1;
        match server.source_index(node) {
            None => why = format!("the sound server has no source {node}"),
            Some(target) => {
                let streams = server.own_streams();
                let mut refused = false;
                for stream in streams.iter().filter(|stream| stream.source != target) {
                    if server.move_stream(stream.index, node) {
                        moved += 1;
                        crate::debug_log::dlog!(
                            Debug,
                            "audio.route",
                            "moved the capture stream",
                            stream = stream.index,
                            from_source = stream.source,
                            to_source = target,
                            to = node,
                            polls = polls,
                        );
                    } else {
                        refused = true;
                        why = format!("the sound server refused to move stream {}", stream.index);
                        crate::debug_log::dlog!(
                            Info,
                            "audio.route",
                            "move refused",
                            stream = stream.index,
                            to = node,
                            polls = polls,
                        );
                    }
                }
                // A move is confirmed by listing again, not by its exit code.
                let after = if moved > 0 {
                    server.own_streams()
                } else {
                    streams
                };
                if !after.is_empty() && after.iter().all(|stream| stream.source == target) {
                    return Ok(Routed {
                        moved,
                        polls,
                        waited,
                    });
                }
                if !after.is_empty() && !refused {
                    why = "the stream is not on the chosen source after the move".to_owned();
                }
            }
        }
        if waited >= deadline {
            return Err(why);
        }
        server.pause(poll);
        waited += poll;
    }
}

/// [`route_until_confirmed`] against the real sound server. Blocks up to
/// [`STREAM_DEADLINE`]; capture is gated until it returns.
pub(crate) fn route_confirmed(node: &str) -> Result<(), String> {
    let started = Instant::now();
    let result = route_until_confirmed(&mut Pactl, node, STREAM_DEADLINE, STREAM_POLL);
    match &result {
        Ok(routed) => crate::debug_log::dlog!(
            Info,
            "audio.route",
            "capture stream confirmed on the chosen microphone",
            node = node,
            moved = routed.moved,
            polls = routed.polls,
            confirm_ms = started.elapsed().as_millis() as u64,
        ),
        Err(error) => crate::debug_log::dlog!(
            Info,
            "audio.route",
            "capture stream NOT on the chosen microphone",
            node = node,
            reason = error,
            confirm_ms = started.elapsed().as_millis() as u64,
        ),
    }
    result.map(|_| ())
}

struct Pactl;

impl SoundServer for Pactl {
    fn own_streams(&mut self) -> Vec<OwnStream> {
        own_streams()
    }

    fn source_index(&mut self, node: &str) -> Option<u32> {
        #[derive(serde::Deserialize)]
        struct Source {
            index: u32,
            name: String,
        }
        let sources: Vec<Source> =
            serde_json::from_str(&pactl(&["-f", "json", "list", "sources"])?).ok()?;
        sources
            .into_iter()
            .find(|source| source.name == node)
            .map(|source| source.index)
    }

    fn move_stream(&mut self, stream: u32, node: &str) -> bool {
        pactl(&["move-source-output", &stream.to_string(), node]).is_some()
    }

    fn pause(&mut self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

fn own_streams() -> Vec<OwnStream> {
    let pid = std::process::id().to_string();
    pactl(&["-f", "json", "list", "source-outputs"])
        .map(|json| streams_of(&json, &pid))
        .unwrap_or_default()
}

/// The capture streams in `json` (`pactl list source-outputs`) that belong
/// to the process `pid`.
fn streams_of(json: &str, pid: &str) -> Vec<OwnStream> {
    #[derive(serde::Deserialize)]
    struct Output {
        index: u32,
        #[serde(default)]
        source: u32,
        #[serde(default)]
        properties: std::collections::HashMap<String, serde_json::Value>,
    }
    serde_json::from_str::<Vec<Output>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter(|output| {
            output
                .properties
                .get(PID_PROPERTY)
                .and_then(|value| value.as_str())
                == Some(pid)
        })
        .map(|output| OwnStream {
            index: output.index,
            source: output.source,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "bluez_input.AA_BB_CC_DD_EE_FF.0";

    /// A sound server where the capture stream appears after `appears_after`
    /// polls on the default source (index 1), and the earbuds are source 7.
    struct Fake {
        sources: Vec<(&'static str, u32)>,
        stream_on: Option<u32>,
        appears_after: u32,
        polls: u32,
        refuse_moves: bool,
        move_does_nothing: bool,
        moves: Vec<(u32, String)>,
        slept: Duration,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                sources: vec![("default.monitor", 1), (NODE, 7)],
                stream_on: Some(1),
                appears_after: 0,
                polls: 0,
                refuse_moves: false,
                move_does_nothing: false,
                moves: Vec::new(),
                slept: Duration::ZERO,
            }
        }
    }

    impl SoundServer for Fake {
        fn own_streams(&mut self) -> Vec<OwnStream> {
            if self.polls <= self.appears_after {
                return Vec::new();
            }
            self.stream_on
                .map(|source| OwnStream { index: 42, source })
                .into_iter()
                .collect()
        }
        fn source_index(&mut self, node: &str) -> Option<u32> {
            self.polls += 1;
            self.sources
                .iter()
                .find(|(name, _)| *name == node)
                .map(|(_, index)| *index)
        }
        fn move_stream(&mut self, stream: u32, node: &str) -> bool {
            self.moves.push((stream, node.to_owned()));
            if self.refuse_moves {
                return false;
            }
            if !self.move_does_nothing {
                self.stream_on = self.sources.iter().find(|s| s.0 == node).map(|s| s.1);
            }
            true
        }
        fn pause(&mut self, how_long: Duration) {
            self.slept += how_long;
        }
    }

    fn route(server: &mut Fake) -> Result<Routed, String> {
        route_until_confirmed(
            server,
            NODE,
            Duration::from_secs(2),
            Duration::from_millis(50),
        )
    }

    #[test]
    fn a_stream_on_the_default_is_moved_and_confirmed() {
        let mut server = Fake::new();
        let routed = route(&mut server).unwrap();
        assert_eq!(routed.moved, 1);
        assert_eq!(server.moves, [(42, NODE.to_owned())]);
        assert_eq!(server.stream_on, Some(7));
    }

    #[test]
    fn a_stream_that_appears_late_is_waited_for() {
        let mut server = Fake::new();
        server.appears_after = 3;
        let routed = route(&mut server).unwrap();
        assert!(routed.polls > 3);
        assert_eq!(
            routed.waited,
            Duration::from_millis(50) * (routed.polls - 1)
        );
    }

    #[test]
    fn a_stream_already_on_the_source_needs_no_move() {
        let mut server = Fake::new();
        server.stream_on = Some(7);
        let routed = route(&mut server).unwrap();
        assert_eq!(routed.moved, 0);
        assert!(server.moves.is_empty());
    }

    #[test]
    fn no_stream_ever_appearing_fails_loudly() {
        let mut server = Fake::new();
        server.appears_after = u32::MAX;
        let error = route(&mut server).unwrap_err();
        assert!(error.contains("no capture stream"), "{error}");
        assert!(server.slept >= Duration::from_secs(2));
    }

    #[test]
    fn a_refused_move_fails_with_the_reason() {
        let mut server = Fake::new();
        server.refuse_moves = true;
        let error = route(&mut server).unwrap_err();
        assert!(error.contains("refused"), "{error}");
    }

    #[test]
    fn a_move_that_does_not_take_is_not_confirmed() {
        let mut server = Fake::new();
        server.move_does_nothing = true;
        let error = route(&mut server).unwrap_err();
        assert!(error.contains("not on the chosen source"), "{error}");
    }

    #[test]
    fn a_missing_source_fails_naming_it() {
        let mut server = Fake::new();
        server.sources.retain(|(name, _)| *name != NODE);
        let error = route(&mut server).unwrap_err();
        assert!(error.contains(NODE), "{error}");
    }

    #[test]
    fn only_this_process_s_streams_are_listed() {
        let json = r#"[
          {"index": 5, "source": 1, "properties": {"hex.process.id": "100"}},
          {"index": 6, "source": 1, "properties": {"hex.process.id": "200"}},
          {"index": 7, "source": 9, "properties": {}}
        ]"#;
        assert_eq!(
            streams_of(json, "100"),
            [OwnStream {
                index: 5,
                source: 1
            }]
        );
        assert!(streams_of("not json", "100").is_empty());
    }
}
