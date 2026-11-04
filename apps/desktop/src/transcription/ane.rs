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
    let verdict = (cfg!(target_arch = "aarch64") && crate::labs::neural_engine())
        .then(|| probe::current().verdict);
    offer_for(verdict.as_ref())
}

/// `None` when the toggle is off or the platform cannot have the engine.
#[cfg(target_os = "linux")]
fn offer_for(verdict: Option<&probe::Verdict>) -> Offer {
    match verdict {
        None | Some(probe::Verdict::Unavailable(_)) => Offer::Hidden,
        Some(probe::Verdict::NotPrepared) => Offer::NotPrepared,
        Some(probe::Verdict::Available) => Offer::Ready,
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

/// The supervisor that most recently started owns the published state, so a
/// retiring one (a model swap) cannot overwrite its successor's.
#[cfg(target_os = "linux")]
static LATEST_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "linux")]
pub(crate) fn register() -> u64 {
    LATEST_OWNER.fetch_add(1, Ordering::AcqRel) + 1
}

#[cfg(target_os = "linux")]
pub(crate) fn publish(owner: u64, state: worker::State) {
    if LATEST_OWNER.load(Ordering::Acquire) != owner {
        return;
    }
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn the_offer_follows_the_toggle_and_the_probe() {
        use probe::Verdict;
        assert_eq!(
            offer_for(None),
            Offer::Hidden,
            "toggle off or wrong platform"
        );
        assert_eq!(
            offer_for(Some(&Verdict::Unavailable("no device".into()))),
            Offer::Hidden
        );
        assert_eq!(offer_for(Some(&Verdict::NotPrepared)), Offer::NotPrepared);
        assert_eq!(offer_for(Some(&Verdict::Available)), Offer::Ready);
    }

    #[test]
    fn a_retired_supervisor_cannot_overwrite_its_successor_on_the_card() {
        let old = register();
        let new = register();
        publish(new, worker::State::Starting);
        publish(old, worker::State::Degraded);
        assert_eq!(LIVE.load(Ordering::Acquire), LIVE_STARTING);
        publish(new, worker::State::Ready);
        assert_eq!(LIVE.load(Ordering::Acquire), LIVE_READY);
    }
}
