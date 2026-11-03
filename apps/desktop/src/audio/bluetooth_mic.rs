//! Dictating through Bluetooth earbuds on Linux. Earbuds in music mode (an
//! A2DP profile) have no microphone: PipeWire makes one only in headset
//! mode (an HSP/HFP profile), which sounds like a phone call. The flow is
//! to switch the earbuds to headset mode when a dictation starts, wait for
//! their microphone to appear, and put the profile back when it is over.
//!
//! This is the pure state machine over a [`Controller`] (PipeWire) and a
//! [`MarkerStore`] (a file that survives a crash); `crate::linux::headset`
//! drives it with `pactl`.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::time::Duration;

use crate::app_settings::BluetoothMic;
use crate::audio::sources::{Device, is_music_profile};

/// How long the earbuds' microphone has to appear after the first (wideband)
/// headset profile, and after the fallback.
pub const FIRST_WAIT: Duration = Duration::from_millis(2000);
pub const FALLBACK_WAIT: Duration = Duration::from_millis(1500);
const STEP: Duration = Duration::from_millis(50);
const RESTORE_ATTEMPTS: usize = 3;
const RESTORE_RETRY: Duration = Duration::from_millis(200);

/// What the sound server says about one card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardState {
    pub active: Option<String>,
    /// Headset profiles the card offers, best first.
    pub headset_profiles: Vec<String>,
}

/// The sound server, as far as the flow needs it. Tests fake it; nothing
/// else may touch the real audio system.
pub trait Controller {
    fn card(&mut self, card: &str) -> Result<CardState, String>;
    fn set_profile(&mut self, card: &str, profile: &str) -> Result<(), String>;
    /// Whether the earbuds' microphone exists now.
    fn source_present(&mut self, device_id: &str) -> bool;
    fn pause(&mut self, how_long: Duration);
}

/// A profile switch that was started and not yet undone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Marker {
    pub card: String,
    pub previous: String,
}

/// Remembers a switch outside the process, so a crash cannot leave the
/// earbuds in headset mode for good.
pub trait MarkerStore {
    fn load(&mut self) -> Option<Marker>;
    fn save(&mut self, marker: &Marker);
    fn clear(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Begun {
    /// The microphone already exists and nothing was changed.
    AlreadyHeadset,
    /// The card was switched; restore it when done.
    Switched,
    /// An earlier dictation's switch is still in place.
    Reused,
}

pub struct Machine<C, M> {
    controller: C,
    marker: M,
    active: Option<Marker>,
}

impl<C: Controller, M: MarkerStore> Machine<C, M> {
    pub fn new(controller: C, marker: M) -> Self {
        Self {
            controller,
            marker,
            active: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Puts the earbuds in headset mode and waits for their microphone.
    /// `device_id` is the earbuds' `bluez_input.<address>` id.
    pub fn begin(&mut self, card: &str, device_id: &str) -> Result<Begun, String> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.card == card)
            && self.controller.source_present(device_id)
        {
            return Ok(Begun::Reused);
        }
        let state = self.controller.card(card)?;
        let Some(active) = state.active.clone() else {
            return Err("the earbuds are not connected".into());
        };
        if !is_music_profile(&active) {
            // Already in headset mode (or off): the microphone is, or is not,
            // there without our help.
            return if self.controller.source_present(device_id) {
                Ok(Begun::AlreadyHeadset)
            } else {
                Err("the earbuds are not in a mode with a microphone".into())
            };
        }
        if state.headset_profiles.is_empty() {
            return Err("the earbuds have no headset mode".into());
        }
        // The marker goes first: a crash after the switch must still be undone.
        let marker = Marker {
            card: card.to_owned(),
            previous: active,
        };
        self.marker.save(&marker);
        self.active = Some(marker);
        let mut last_error = String::from("the earbuds' microphone did not appear");
        for (attempt, profile) in state.headset_profiles.iter().enumerate() {
            if let Err(error) = self.controller.set_profile(card, profile) {
                last_error = error;
                continue;
            }
            let wait = if attempt == 0 {
                FIRST_WAIT
            } else {
                FALLBACK_WAIT
            };
            if self.wait_for_source(device_id, wait) {
                return Ok(Begun::Switched);
            }
            last_error = format!("the earbuds' microphone did not appear in {profile}");
        }
        self.restore();
        Err(last_error)
    }

    fn wait_for_source(&mut self, device_id: &str, limit: Duration) -> bool {
        let mut waited = Duration::ZERO;
        loop {
            if self.controller.source_present(device_id) {
                return true;
            }
            if waited >= limit {
                return false;
            }
            self.controller.pause(STEP);
            waited += STEP;
        }
    }

    /// Puts the earbuds back in the profile they were in. Safe to call when
    /// nothing was switched. A failed restore keeps the marker, so the next
    /// start tries again.
    pub fn restore(&mut self) {
        let Some(marker) = self.active.take() else {
            return;
        };
        if self.put_back(&marker) {
            self.marker.clear();
        }
    }

    fn put_back(&mut self, marker: &Marker) -> bool {
        for attempt in 0..RESTORE_ATTEMPTS {
            if attempt > 0 {
                self.controller.pause(RESTORE_RETRY);
            }
            if self
                .controller
                .set_profile(&marker.card, &marker.previous)
                .is_ok()
            {
                return true;
            }
        }
        false
    }

    /// At startup: undoes a switch an earlier run left behind.
    pub fn recover_stale(&mut self) {
        let Some(marker) = self.marker.load() else {
            return;
        };
        match self.controller.card(&marker.card) {
            // The card is in a headset mode we put it in, or never left music mode.
            Ok(state) => {
                let stranded = state
                    .active
                    .as_deref()
                    .is_some_and(|profile| !is_music_profile(profile));
                if !stranded || self.put_back(&marker) {
                    self.marker.clear();
                }
            }
            // The earbuds are gone; there is nothing to restore.
            Err(_) => self.marker.clear(),
        }
    }
}

/// What starting a dictation does about Bluetooth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Plan {
    /// Switch this earbuds card to headset mode first.
    Headset {
        card: String,
        device_id: String,
    },
    Nothing,
}

/// The selected microphone, when it is earbuds in music mode and the choice
/// is to use their microphone. Other cases need no switch: with
/// `KeepQuality` input resolution passes over the earbuds for another
/// microphone and leaves them alone.
pub fn plan(mode: BluetoothMic, selected: Option<&str>, devices: &[Device]) -> Plan {
    let Some(device) = selected.and_then(|id| devices.iter().find(|device| device.id == id)) else {
        return Plan::Nothing;
    };
    match (mode, &device.bluetooth) {
        (BluetoothMic::Headset, Some(card)) if device.needs_headset_mode() => Plan::Headset {
            card: card.card.clone(),
            device_id: device.id.clone(),
        },
        _ => Plan::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::sources::{BluetoothCard, DeviceKind};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    const CARD: &str = "bluez_card.00_11_22_33_44_55";
    const ID: &str = "bluez_input.00:11:22:33:44:55";

    #[derive(Default)]
    struct World {
        profiles: HashMap<String, String>,
        offered: Vec<String>,
        /// Profiles that make the microphone appear after `delay` polls.
        mic_in: Vec<(String, u32)>,
        polls_since_switch: u32,
        calls: Vec<String>,
        fail_set: Vec<String>,
        marker: Option<Marker>,
        gone: bool,
        slept: Duration,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<RefCell<World>>);

    impl Fake {
        fn earbuds(profile: &str) -> Self {
            let fake = Self::default();
            {
                let mut world = fake.0.borrow_mut();
                world.profiles.insert(CARD.into(), profile.into());
                world.offered = vec!["headset-head-unit".into(), "headset-head-unit-cvsd".into()];
                world.mic_in = vec![("headset-head-unit".into(), 3)];
            }
            fake
        }
        fn profile(&self) -> String {
            self.0.borrow().profiles[CARD].clone()
        }
        fn calls(&self) -> Vec<String> {
            self.0.borrow().calls.clone()
        }
    }

    impl Controller for Fake {
        fn card(&mut self, card: &str) -> Result<CardState, String> {
            let world = self.0.borrow();
            if world.gone {
                return Err("no such card".into());
            }
            Ok(CardState {
                active: world.profiles.get(card).cloned(),
                headset_profiles: world.offered.clone(),
            })
        }
        fn set_profile(&mut self, card: &str, profile: &str) -> Result<(), String> {
            let mut world = self.0.borrow_mut();
            world.calls.push(format!("set {profile}"));
            if world.fail_set.iter().any(|failing| failing == profile) {
                return Err("refused".into());
            }
            world.profiles.insert(card.into(), profile.into());
            world.polls_since_switch = 0;
            Ok(())
        }
        fn source_present(&mut self, _: &str) -> bool {
            let mut world = self.0.borrow_mut();
            world.polls_since_switch += 1;
            let profile = world.profiles[CARD].clone();
            world
                .mic_in
                .iter()
                .any(|(wanted, delay)| *wanted == profile && world.polls_since_switch > *delay)
        }
        fn pause(&mut self, how_long: Duration) {
            self.0.borrow_mut().slept += how_long;
        }
    }

    #[derive(Clone, Default)]
    struct FakeMarker(Rc<RefCell<Option<Marker>>>);

    impl MarkerStore for FakeMarker {
        fn load(&mut self) -> Option<Marker> {
            self.0.borrow().clone()
        }
        fn save(&mut self, marker: &Marker) {
            *self.0.borrow_mut() = Some(marker.clone());
        }
        fn clear(&mut self) {
            *self.0.borrow_mut() = None;
        }
    }

    fn machine(fake: &Fake, marker: &FakeMarker) -> Machine<Fake, FakeMarker> {
        Machine::new(fake.clone(), marker.clone())
    }

    #[test]
    fn music_mode_switches_to_headset_waits_and_restores() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        let mut machine = machine(&fake, &marker);
        assert_eq!(machine.begin(CARD, ID), Ok(Begun::Switched));
        assert_eq!(fake.profile(), "headset-head-unit");
        assert!(machine.is_active());
        assert_eq!(
            marker.0.borrow().as_ref().map(|m| m.previous.as_str()),
            Some("a2dp-sink"),
            "the marker names the profile to return to"
        );
        machine.restore();
        assert_eq!(fake.profile(), "a2dp-sink");
        assert!(!machine.is_active());
        assert!(
            marker.0.borrow().is_none(),
            "a clean restore clears the marker"
        );
        // A second restore is harmless.
        machine.restore();
        assert_eq!(fake.calls(), ["set headset-head-unit", "set a2dp-sink"]);
    }

    #[test]
    fn the_microphone_wait_is_bounded_and_falls_back_to_cvsd() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        fake.0.borrow_mut().mic_in = vec![("headset-head-unit-cvsd".into(), 2)];
        let mut machine = machine(&fake, &marker);
        assert_eq!(machine.begin(CARD, ID), Ok(Begun::Switched));
        assert_eq!(fake.profile(), "headset-head-unit-cvsd");
        let slept = fake.0.borrow().slept;
        assert!(
            slept >= FIRST_WAIT,
            "waited the whole first window: {slept:?}"
        );
        assert!(slept < FIRST_WAIT + FALLBACK_WAIT);
    }

    #[test]
    fn a_microphone_that_never_appears_times_out_and_restores() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        fake.0.borrow_mut().mic_in.clear();
        let mut machine = machine(&fake, &marker);
        let error = machine.begin(CARD, ID).unwrap_err();
        assert!(error.contains("did not appear"), "{error}");
        assert_eq!(fake.profile(), "a2dp-sink", "restored after the timeout");
        assert!(!machine.is_active());
        assert!(marker.0.borrow().is_none());
        let slept = fake.0.borrow().slept;
        assert!(slept <= FIRST_WAIT + FALLBACK_WAIT + Duration::from_secs(1));
    }

    #[test]
    fn a_refused_switch_restores_and_reports() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        fake.0.borrow_mut().fail_set =
            vec!["headset-head-unit".into(), "headset-head-unit-cvsd".into()];
        let mut machine = machine(&fake, &marker);
        assert_eq!(machine.begin(CARD, ID), Err("refused".into()));
        assert_eq!(fake.profile(), "a2dp-sink");
        assert!(marker.0.borrow().is_none());
    }

    #[test]
    fn earbuds_already_in_headset_mode_are_left_alone() {
        let (fake, marker) = (Fake::earbuds("headset-head-unit"), FakeMarker::default());
        fake.0.borrow_mut().polls_since_switch = 10;
        let mut machine = machine(&fake, &marker);
        assert_eq!(machine.begin(CARD, ID), Ok(Begun::AlreadyHeadset));
        machine.restore();
        assert!(
            fake.calls().is_empty(),
            "nothing switched, nothing restored"
        );
        assert!(marker.0.borrow().is_none());
    }

    #[test]
    fn a_second_dictation_reuses_the_switch_still_in_place() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        let mut machine = machine(&fake, &marker);
        assert_eq!(machine.begin(CARD, ID), Ok(Begun::Switched));
        assert_eq!(machine.begin(CARD, ID), Ok(Begun::Reused));
        machine.restore();
        assert_eq!(fake.calls(), ["set headset-head-unit", "set a2dp-sink"]);
    }

    #[test]
    fn a_cancel_or_error_restores_the_same_way() {
        // The listener restores on finish, cancel and error through the same call.
        for _ in 0..3 {
            let (fake, marker) = (Fake::earbuds("a2dp-sink-sbc_xq"), FakeMarker::default());
            let mut machine = machine(&fake, &marker);
            machine.begin(CARD, ID).unwrap();
            machine.restore();
            assert_eq!(
                fake.profile(),
                "a2dp-sink-sbc_xq",
                "the exact codec profile returns"
            );
        }
    }

    #[test]
    fn a_failed_restore_keeps_the_marker_for_the_next_start() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        let mut machine = machine(&fake, &marker);
        machine.begin(CARD, ID).unwrap();
        fake.0.borrow_mut().fail_set = vec!["a2dp-sink".into()];
        machine.restore();
        assert_eq!(fake.profile(), "headset-head-unit");
        assert!(marker.0.borrow().is_some(), "kept for recover_stale");
        // Next start: the card is still stranded, and now it can be restored.
        fake.0.borrow_mut().fail_set.clear();
        let mut next = self::machine(&fake, &marker);
        next.recover_stale();
        assert_eq!(fake.profile(), "a2dp-sink");
        assert!(marker.0.borrow().is_none());
    }

    #[test]
    fn a_marker_left_by_a_crash_is_undone_on_the_next_start() {
        let (fake, marker) = (Fake::earbuds("headset-head-unit"), FakeMarker::default());
        *marker.0.borrow_mut() = Some(Marker {
            card: CARD.into(),
            previous: "a2dp-sink".into(),
        });
        machine(&fake, &marker).recover_stale();
        assert_eq!(fake.profile(), "a2dp-sink");
        assert!(marker.0.borrow().is_none());
    }

    #[test]
    fn a_stale_marker_for_a_card_that_never_left_music_mode_changes_nothing() {
        let (fake, marker) = (Fake::earbuds("a2dp-sink"), FakeMarker::default());
        *marker.0.borrow_mut() = Some(Marker {
            card: CARD.into(),
            previous: "a2dp-sink".into(),
        });
        machine(&fake, &marker).recover_stale();
        assert!(fake.calls().is_empty());
        assert!(marker.0.borrow().is_none());

        // And one for earbuds that are gone is just dropped.
        *marker.0.borrow_mut() = Some(Marker {
            card: CARD.into(),
            previous: "a2dp-sink".into(),
        });
        fake.0.borrow_mut().gone = true;
        machine(&fake, &marker).recover_stale();
        assert!(marker.0.borrow().is_none());
    }

    fn device(id: &str, kind: DeviceKind, card: Option<(&str, bool)>) -> Device {
        Device {
            id: id.into(),
            label: id.into(),
            kind,
            bluetooth: card.map(|(profile, source)| BluetoothCard {
                card: CARD.into(),
                profile: Some(profile.into()),
                source: source.then(|| "bluez_input.00_11_22_33_44_55.0".into()),
            }),
            hidden: false,
        }
    }

    #[test]
    fn the_plan_switches_only_earbuds_in_music_mode_that_are_selected() {
        let music = [
            device("builtin", DeviceKind::BuiltIn, None),
            device(ID, DeviceKind::Bluetooth, Some(("a2dp-sink", false))),
        ];
        let want = Plan::Headset {
            card: CARD.into(),
            device_id: ID.into(),
        };
        assert_eq!(plan(BluetoothMic::Headset, Some(ID), &music), want);
        assert_eq!(
            plan(BluetoothMic::Headset, Some("builtin"), &music),
            Plan::Nothing
        );
        assert_eq!(plan(BluetoothMic::Headset, None, &music), Plan::Nothing);
        assert_eq!(
            plan(BluetoothMic::Headset, Some("gone"), &music),
            Plan::Nothing
        );
        assert_eq!(
            plan(BluetoothMic::KeepQuality, Some(ID), &music),
            Plan::Nothing,
            "keep quality leaves the earbuds in music mode"
        );
        let headset = [device(
            ID,
            DeviceKind::Bluetooth,
            Some(("headset-head-unit", true)),
        )];
        assert_eq!(
            plan(BluetoothMic::Headset, Some(ID), &headset),
            Plan::Nothing
        );
    }
}
