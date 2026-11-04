//! The Apple Neural Engine Parakeet engine (Linux on Apple silicon): an
//! optional, user-installed runtime that WordInk drives through one resident
//! worker process. Nothing here is bundled or linked; see `probe` for how
//! WordInk decides the engine can exist on this machine.
//!
//! This file is the engine's front door for the rest of the app: whether the
//! model is offered, whether it can be chosen, and what its card says.

use std::sync::atomic::{AtomicU8, Ordering};

// Only Linux on Apple silicon can run the engine; the other targets compile
// just the front door below.
#[cfg(target_os = "linux")]
pub(crate) mod backend;
#[cfg(target_os = "linux")]
pub(crate) mod probe;
#[cfg(target_os = "linux")]
pub(crate) mod protocol;
#[cfg(target_os = "linux")]
pub(crate) mod worker;

/// Whether the model is in the picker, and whether it can be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum Offer {
    /// Not on this machine, or its Labs toggle is off.
    Hidden,
    /// The runtime works but its reference model is not downloaded yet.
    NotPrepared,
    Ready,
}

/// The Labs toggle and the cached probe, nothing slower. Off the target
/// platform it never reads the filesystem.
#[cfg(target_os = "linux")]
pub(crate) fn offer() -> Offer {
    if !cfg!(target_arch = "aarch64") || !crate::labs::neural_engine() {
        return Offer::Hidden;
    }
    match probe::current().verdict {
        probe::Verdict::Unavailable(_) => Offer::Hidden,
        probe::Verdict::NotPrepared => Offer::NotPrepared,
        probe::Verdict::Available => Offer::Ready,
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn offer() -> Offer {
    Offer::Hidden
}

/// The worker's state as the card shows it. One engine owns one worker, so a
/// process-wide value is enough; the last writer wins.
static LIVE: AtomicU8 = AtomicU8::new(LIVE_NONE);
const LIVE_NONE: u8 = 0;
const LIVE_STARTING: u8 = 1;
const LIVE_READY: u8 = 2;
const LIVE_DEGRADED: u8 = 3;

#[cfg(target_os = "linux")]
pub(crate) fn publish(state: worker::State) {
    let value = match state {
        worker::State::Stopped => LIVE_NONE,
        worker::State::Starting => LIVE_STARTING,
        worker::State::Ready => LIVE_READY,
        worker::State::Degraded => LIVE_DEGRADED,
    };
    LIVE.store(value, Ordering::Release);
}

/// What the model's card adds to its detail line, or `None` when it has
/// nothing to say.
pub(crate) fn card_note() -> Option<&'static str> {
    match offer() {
        Offer::Hidden => None,
        Offer::NotPrepared => Some(NOT_PREPARED_NOTE),
        Offer::Ready => match LIVE.load(Ordering::Acquire) {
            LIVE_STARTING => Some("Warming up; the GPU model answers meanwhile"),
            LIVE_DEGRADED => Some("Paused after an error; the GPU model answers meanwhile"),
            _ => None,
        },
    }
}

/// The card names the runtime's own command that prepares the reference model
/// (`mlx-omarchy-parakeet download`); WordInk never downloads it.
const NOT_PREPARED_NOTE: &str =
    "Not ready: download its reference model first with mlx-omarchy-parakeet download";
