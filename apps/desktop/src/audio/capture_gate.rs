//! Holds a capture stream's audio back until the stream is known to be on the
//! microphone that was asked for. On Linux Hex opens ALSA's default device and
//! then moves its own stream to the chosen PipeWire source; audio read before
//! the move confirms comes from the wrong device (often a silent monitor), so
//! the stream callback drops it instead of buffering it. A move that never
//! confirms fails the open, so a capture never records the wrong source.
//!
//! Other platforms open the right device directly and use an open gate.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

const OPEN: u8 = 0;
const WAITING: u8 = 1;
const FAILED: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateState {
    /// Audio flows through.
    Open,
    /// The stream is not yet on its device: audio is dropped.
    Waiting,
    /// The stream could not be put on its device: audio is dropped.
    Failed,
}

#[derive(Debug, Default)]
struct Shared {
    state: AtomicU8,
    dropped_frames: AtomicU64,
}

/// Shared between the stream callback and whoever confirms the route.
#[derive(Clone, Debug, Default)]
pub struct CaptureGate(Arc<Shared>);

impl CaptureGate {
    /// A gate that passes audio: the stream opened on its device directly.
    pub fn open() -> Self {
        Self::default()
    }

    /// A gate that drops audio until [`Self::confirm`].
    pub fn waiting() -> Self {
        let gate = Self::default();
        gate.0.state.store(WAITING, Ordering::Release);
        gate
    }

    pub fn state(&self) -> GateState {
        match self.0.state.load(Ordering::Acquire) {
            OPEN => GateState::Open,
            WAITING => GateState::Waiting,
            _ => GateState::Failed,
        }
    }

    /// The stream is on its device: audio flows from the next callback.
    pub fn confirm(&self) {
        self.0.state.store(OPEN, Ordering::Release);
    }

    /// The stream will never reach its device.
    pub fn fail(&self) {
        self.0.state.store(FAILED, Ordering::Release);
    }

    /// Called per callback with its frame count; whether to pass the audio on.
    /// Dropped frames are counted for the debug log.
    pub fn passes(&self, frames: usize) -> bool {
        if self.0.state.load(Ordering::Acquire) == OPEN {
            return true;
        }
        self.0
            .dropped_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        false
    }

    /// How many frames were dropped while the gate was closed.
    pub fn dropped_frames(&self) -> u64 {
        self.0.dropped_frames.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_is_dropped_not_buffered_until_the_route_confirms() {
        let gate = CaptureGate::waiting();
        assert_eq!(gate.state(), GateState::Waiting);
        assert!(!gate.passes(480));
        assert!(!gate.passes(480));
        assert_eq!(gate.dropped_frames(), 960);
        gate.confirm();
        assert_eq!(gate.state(), GateState::Open);
        assert!(gate.passes(480));
        assert_eq!(gate.dropped_frames(), 960, "open audio is not counted");
    }

    #[test]
    fn a_failed_route_never_passes_audio() {
        let gate = CaptureGate::waiting();
        gate.fail();
        assert_eq!(gate.state(), GateState::Failed);
        assert!(!gate.passes(480));
    }

    #[test]
    fn an_open_gate_passes_everything() {
        let gate = CaptureGate::open();
        assert!(gate.passes(1));
        assert_eq!(gate.dropped_frames(), 0);
    }

    #[test]
    fn clones_share_one_state() {
        let gate = CaptureGate::waiting();
        let seen_by_stream = gate.clone();
        gate.confirm();
        assert!(seen_by_stream.passes(10));
    }
}
