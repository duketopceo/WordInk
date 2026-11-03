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
const STREAM_DEADLINE: Duration = Duration::from_millis(3_000);
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
    /// The node name and owning application (when it says) of the source
    /// with this index, to name whoever took the stream.
    fn source_owner(&mut self, source: u32) -> Option<SourceOwner>;
    fn pause(&mut self, how_long: Duration);
}

/// What a source is, as far as naming it to the user goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceOwner {
    pub(crate) node: String,
    pub(crate) application: Option<String>,
}

/// The sound server's marker for a stream not linked to any source yet.
const UNLINKED: u32 = u32::MAX;
/// A route counts as confirmed only after the stream stayed on the source
/// for this many polls in a row, over at least this long: apps such as
/// EasyEffects move every new capture stream into their own chain a moment
/// after it is placed.
const STABLE_POLLS: u32 = 3;
/// A stream that already followed the default source, with no move of ours
/// to undo, needs less: there is no re-link race of our own making.
const STABLE_POLLS_UNMOVED: u32 = 2;
const STABLE_FOR_UNMOVED: Duration = Duration::from_millis(150);
const UNLINKED_GRACE_POLLS: u32 = 5;
const STABLE_FOR: Duration = Duration::from_millis(300);

/// The message for a stream another program keeps taking off the chosen
/// microphone, naming that program when it is a known one.
pub(crate) fn culprit_message(owner: Option<&SourceOwner>, chosen: &str) -> String {
    let named = owner.and_then(|owner| {
        let text = format!(
            "{} {}",
            owner.node.to_lowercase(),
            owner.application.as_deref().unwrap_or("").to_lowercase()
        );
        [
            (
                "easyeffects",
                "EasyEffects",
                "EasyEffects \u{203a} Input, or turn off Process All Inputs",
            ),
            (
                "pulseeffects",
                "PulseEffects",
                "PulseEffects, or turn off its input processing",
            ),
            ("noisetorch", "NoiseTorch", "NoiseTorch, or turn it off"),
        ]
        .into_iter()
        .find(|(key, _, _)| text.contains(key))
    });
    match (named, owner) {
        (Some((_, name, fix)), _) => {
            format!("{name} is capturing WordInk's microphone \u{2014} exclude WordInk in {fix}")
        }
        (None, Some(owner)) => format!(
            "{} keeps moving WordInk's microphone away from {chosen} \u{2014} exclude WordInk from it",
            owner.application.as_deref().unwrap_or(&owner.node)
        ),
        (None, None) => format!(
            "something keeps moving WordInk's microphone away from {chosen}; the capture was stopped"
        ),
    }
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
    // Where the stream was last seen after our move, if not on the target.
    let mut bounced_to: Option<u32> = None;
    // Whether the server ever listed the stream on the target.
    let mut seen_on_target = false;
    let mut stable_polls = 0;
    let mut stable_since = Duration::ZERO;
    loop {
        polls += 1;
        match server.source_index(node) {
            None => {
                why = format!("the sound server has no source {node}");
                stable_polls = 0;
            }
            Some(target) => {
                let streams = server.own_streams();
                let mut refused = false;
                let mut moved_now = false;
                for stream in streams.iter().filter(|stream| stream.source != target) {
                    // A stream still being linked gets a moment to follow the
                    // default source before it is moved by hand.
                    if stream.source == UNLINKED && polls <= UNLINKED_GRACE_POLLS {
                        continue;
                    }
                    if seen_on_target && stream.source != UNLINKED {
                        // Placed there by us earlier and found elsewhere now.
                        bounced_to = Some(stream.source);
                        crate::debug_log::dlog!(
                            Info,
                            "audio.route",
                            "rerouted",
                            stream = stream.index,
                            now_on_source = stream.source,
                            polls = polls,
                        );
                    }
                    if server.move_stream(stream.index, node) {
                        moved += 1;
                        moved_now = true;
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
                // A move is confirmed by listing again, not by its exit code,
                // and only a placement that holds counts.
                let after = if moved_now {
                    server.own_streams()
                } else {
                    streams
                };
                let on_target =
                    !after.is_empty() && after.iter().all(|stream| stream.source == target);
                seen_on_target |= on_target;
                if on_target && !moved_now {
                    if stable_polls == 0 {
                        stable_since = waited;
                    }
                    stable_polls += 1;
                    let (need_polls, need_for) = if moved == 0 {
                        (STABLE_POLLS_UNMOVED, STABLE_FOR_UNMOVED)
                    } else {
                        (STABLE_POLLS, STABLE_FOR)
                    };
                    if stable_polls >= need_polls && waited - stable_since >= need_for {
                        return Ok(Routed {
                            moved,
                            polls,
                            waited,
                        });
                    }
                } else {
                    stable_polls = 0;
                    if !on_target && !after.is_empty() && !refused {
                        why = "the stream is not on the chosen source after the move".to_owned();
                    }
                    if seen_on_target
                        && !on_target
                        && let Some(other) = after
                            .iter()
                            .find(|s| s.source != target && s.source != UNLINKED)
                    {
                        bounced_to = Some(other.source);
                    }
                }
            }
        }
        if waited >= deadline {
            return Err(match bounced_to {
                Some(source) => {
                    let owner = server.source_owner(source);
                    crate::debug_log::dlog!(
                        Info,
                        "audio.route",
                        "another program keeps rerouting the stream",
                        node = node,
                        owner_node = owner.as_ref().map(|o| o.node.clone()),
                        owner_app = owner.as_ref().and_then(|o| o.application.clone()),
                    );
                    culprit_message(owner.as_ref(), node)
                }
                None => why,
            });
        }
        server.pause(poll);
        waited += poll;
    }
}

/// While a capture runs: whether this process's streams are still on the
/// source `node`. A stream found elsewhere means another program took it, and
/// the error names that program. No stream at all is not a failure (the
/// capture just closed).
pub(crate) fn check_still_routed(server: &mut impl SoundServer, node: &str) -> Result<(), String> {
    let Some(target) = server.source_index(node) else {
        return Err(format!("the sound server no longer has {node}"));
    };
    let Some(stray) = server
        .own_streams()
        .into_iter()
        .find(|stream| stream.source != target)
    else {
        return Ok(());
    };
    let owner = (stray.source != UNLINKED)
        .then(|| server.source_owner(stray.source))
        .flatten();
    crate::debug_log::dlog!(
        Info,
        "audio.route",
        "rerouted",
        stream = stray.index,
        now_on_source = stray.source,
        node_name = owner.as_ref().map(|o| o.node.clone()),
        owner_app = owner.as_ref().and_then(|o| o.application.clone()),
    );
    Err(culprit_message(owner.as_ref(), node))
}

/// Checks every second that the capture stream stays on `node`; on a drift it
/// fails `gate` with the culprit message, which ends the capture loudly.
pub(crate) fn watch_route(
    node: String,
    gate: crate::audio::capture_gate::CaptureGate,
) -> crate::audio::capture_gate::StopOnDrop {
    let (guard, stop) = crate::audio::capture_gate::StopOnDrop::new();
    let spawned = std::thread::Builder::new()
        .name("route-watch".into())
        .spawn(move || {
            'watching: loop {
                for _ in 0..10 {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                if let Err(message) = check_still_routed(&mut Pactl, &node) {
                    set_route_problem(message.clone());
                    gate.fail_with(message);
                    break 'watching;
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start watching the capture route");
    }
    guard
}

static ROUTE_PROBLEM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn set_route_problem(message: String) {
    *ROUTE_PROBLEM.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
}

/// The reason the last capture stream was taken off its microphone, once.
pub(crate) fn take_route_problem() -> Option<String> {
    ROUTE_PROBLEM
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
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

    fn source_owner(&mut self, source: u32) -> Option<SourceOwner> {
        #[derive(serde::Deserialize)]
        struct Source {
            index: u32,
            name: String,
            #[serde(default)]
            properties: std::collections::HashMap<String, serde_json::Value>,
        }
        let sources: Vec<Source> =
            serde_json::from_str(&pactl(&["-f", "json", "list", "sources"])?).ok()?;
        let found = sources
            .into_iter()
            .find(|candidate| candidate.index == source)?;
        let text = |key: &str| {
            found
                .properties
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        };
        Some(SourceOwner {
            application: text("application.name").or_else(|| text("node.description")),
            node: found.name,
        })
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
        /// EasyEffects-style: takes the stream back this many polls after
        /// it is placed on the earbuds.
        bounce_after: Option<u32>,
        on_target_polls: u32,
        moves: Vec<(u32, String)>,
        slept: Duration,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                sources: vec![
                    ("default.monitor", 1),
                    (NODE, 7),
                    ("easyeffects_source", 123),
                ],
                stream_on: Some(1),
                appears_after: 0,
                polls: 0,
                refuse_moves: false,
                move_does_nothing: false,
                bounce_after: None,
                on_target_polls: 0,
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
            if let Some(after) = self.bounce_after
                && self.stream_on == Some(7)
            {
                self.on_target_polls += 1;
                if self.on_target_polls > after {
                    self.stream_on = Some(123);
                    self.on_target_polls = 0;
                }
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
        fn source_owner(&mut self, source: u32) -> Option<SourceOwner> {
            self.sources
                .iter()
                .find(|(_, index)| *index == source)
                .map(|(name, _)| SourceOwner {
                    node: (*name).to_owned(),
                    application: None,
                })
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
    fn a_stream_that_followed_the_default_confirms_in_a_shorter_window() {
        let mut server = Fake::new();
        server.stream_on = Some(7);
        let routed = route(&mut server).unwrap();
        assert_eq!(routed.moved, 0);
        assert!(routed.waited >= STABLE_FOR_UNMOVED);
        assert!(routed.waited < STABLE_FOR, "{:?}", routed.waited);
        // The watcher still catches a later hijack.
        server.stream_on = Some(123);
        assert!(check_still_routed(&mut server, NODE).is_err());
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

    #[test]
    fn a_stream_that_bounces_back_is_not_falsely_confirmed() {
        // The regression: the stream is listed on the earbuds for a poll or
        // two after each move, then another program takes it back.
        let mut server = Fake::new();
        server.bounce_after = Some(2);
        let error = route(&mut server).unwrap_err();
        assert!(error.contains("EasyEffects"), "{error}");
        assert!(error.contains("exclude WordInk"), "{error}");
        assert!(
            server.moves.len() > 1,
            "it kept trying: {}",
            server.moves.len()
        );
    }

    #[test]
    fn a_placement_must_hold_for_three_polls_over_three_hundred_milliseconds() {
        let mut server = Fake::new();
        server.bounce_after = Some(20);
        // Holds far longer than the stability window, so it confirms.
        let routed = route(&mut server).unwrap();
        assert!(routed.polls > STABLE_POLLS);
        let held = routed.waited;
        assert!(held >= STABLE_FOR, "{held:?}");
        // One that drifts back after a single poll never reaches it.
        let mut quick = Fake::new();
        quick.bounce_after = Some(1);
        assert!(route(&mut quick).is_err());
    }

    #[test]
    fn a_stream_rerouted_mid_capture_is_caught_and_names_the_culprit() {
        let mut server = Fake::new();
        server.stream_on = Some(7);
        assert_eq!(check_still_routed(&mut server, NODE), Ok(()));
        // The other program takes it during the capture.
        server.stream_on = Some(123);
        let error = check_still_routed(&mut server, NODE).unwrap_err();
        assert!(
            error.starts_with("EasyEffects is capturing WordInk's microphone"),
            "{error}"
        );
        // No stream (the capture just closed) is not a failure.
        server.stream_on = None;
        assert_eq!(check_still_routed(&mut server, NODE), Ok(()));
        // The chosen source vanishing is.
        server.sources.retain(|(name, _)| *name != NODE);
        assert!(check_still_routed(&mut server, NODE).is_err());
    }

    #[test]
    fn the_culprit_message_names_known_programs_and_falls_back_for_others() {
        let owner = |node: &str, app: Option<&str>| SourceOwner {
            node: node.to_owned(),
            application: app.map(str::to_owned),
        };
        assert_eq!(
            culprit_message(Some(&owner("easyeffects_source", None)), NODE),
            "EasyEffects is capturing WordInk's microphone \u{2014} exclude WordInk in EasyEffects \u{203a} Input, or turn off Process All Inputs"
        );
        assert!(
            culprit_message(Some(&owner("x", Some("NoiseTorch"))), NODE)
                .starts_with("NoiseTorch is capturing")
        );
        let other = culprit_message(Some(&owner("filter_chain_source", Some("Chain"))), NODE);
        assert!(
            other.starts_with("Chain keeps moving WordInk's microphone"),
            "{other}"
        );
        assert!(culprit_message(None, NODE).contains("something keeps moving"));
    }
}
