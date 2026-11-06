//! The Escape-twice gesture that cancels a recording, shared by every
//! platform's keyboard listener. Pure: callers pass the time and whether a
//! recording is in progress.

use std::time::{Duration, Instant};

/// Two Escape presses this close cancel a recording.
pub(crate) const ESCAPE_CANCEL_WINDOW: Duration = Duration::from_millis(600);
/// An Escape press this soon after the last is the key repeating, not a
/// second press.
pub(crate) const ESCAPE_REPEAT_GAP: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
pub(crate) struct EscapeCancel {
    /// The first Escape press of a possible double press while recording.
    first: Option<Instant>,
    /// The latest Escape press of any kind, to tell a repeating key.
    seen: Option<Instant>,
}

impl EscapeCancel {
    /// A bare Escape press. True when it is the second within
    /// [`ESCAPE_CANCEL_WINDOW`] of the first while recording: cancel.
    pub(crate) fn pressed(&mut self, now: Instant, recording: bool) -> bool {
        let seen = self.seen.replace(now);
        if !recording {
            self.first = None;
            return false;
        }
        // Every press of a held key counts toward the next repeat, so a
        // key held down never reads as a second press.
        if seen.is_some_and(|seen| now.saturating_duration_since(seen) < ESCAPE_REPEAT_GAP) {
            return false;
        }
        match self.first {
            Some(first) if now.saturating_duration_since(first) <= ESCAPE_CANCEL_WINDOW => {
                self.first = None;
                true
            }
            _ => {
                self.first = Some(now);
                false
            }
        }
    }

    /// Any other key press: the pair is broken.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub(crate) fn other_key(&mut self) {
        self.first = None;
    }

    /// The recording ended or input was lost: forget a pending first press.
    pub(crate) fn reset(&mut self) {
        self.first = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn escape_twice_inside_the_window_cancels() {
        let t = Instant::now();
        let mut escape = EscapeCancel::default();
        assert!(!escape.pressed(t, true));
        assert!(escape.pressed(t + ms(300), true));
        // The pair is spent: the next Escape starts a new one.
        assert!(!escape.pressed(t + ms(500), true));
    }

    #[test]
    fn escape_then_another_key_does_not_cancel() {
        let t = Instant::now();
        let mut escape = EscapeCancel::default();
        assert!(!escape.pressed(t, true));
        escape.other_key();
        assert!(!escape.pressed(t + ms(300), true));
    }

    #[test]
    fn a_slow_second_press_only_starts_a_new_pair() {
        let t = Instant::now();
        let mut escape = EscapeCancel::default();
        assert!(!escape.pressed(t, true));
        assert!(!escape.pressed(t + ESCAPE_CANCEL_WINDOW + ms(100), true));
        assert!(escape.pressed(t + ESCAPE_CANCEL_WINDOW + ms(300), true));
    }

    #[test]
    fn escape_while_idle_is_a_no_op_and_arms_nothing() {
        let t = Instant::now();
        let mut escape = EscapeCancel::default();
        assert!(!escape.pressed(t, false));
        assert!(!escape.pressed(t + ms(200), false));
        // Recording starts: the idle presses do not count as a first.
        assert!(!escape.pressed(t + ms(400), true));
    }

    #[test]
    fn a_repeating_key_is_not_a_second_press() {
        let t = Instant::now();
        let mut escape = EscapeCancel::default();
        assert!(!escape.pressed(t, true));
        for i in 1..5 {
            assert!(!escape.pressed(t + ms(30 * i), true));
        }
    }
}
