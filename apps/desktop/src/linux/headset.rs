//! Runs the Bluetooth earbuds flow (`crate::audio::bluetooth_mic`) against
//! the real sound server with `pactl`, on a worker thread so the dictation
//! loop never waits on a profile switch. Dropping the flow puts the earbuds
//! back, so an error that leaves the listener restores them too.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::audio::bluetooth_mic::{CardState, Controller, Machine, Marker, MarkerStore};
use crate::audio::sources;
use crate::linux::pulse::{listing, pactl};

/// The id prefix of a Bluetooth microphone.
pub(crate) const ID_PREFIX: &str = "bluez_input.";

/// Whether a dictation from this saved microphone starts in headset mode.
pub(crate) fn is_managed(settings: &crate::app_settings::AppSettings) -> bool {
    settings.bluetooth_mic == crate::app_settings::BluetoothMic::Headset
        && settings
            .microphone
            .as_deref()
            .is_some_and(|id| id.starts_with(ID_PREFIX))
}

struct Pactl;

impl Controller for Pactl {
    fn card(&mut self, card: &str) -> Result<CardState, String> {
        let json =
            pactl(&["-f", "json", "list", "cards"]).ok_or("the sound server did not answer")?;
        let cards = sources::parse_cards(&json)?;
        let found = cards
            .iter()
            .find(|candidate| candidate.name == card)
            .ok_or("the earbuds are not connected")?;
        let mut offered = Vec::new();
        for profile in ["headset-head-unit", "headset-head-unit-cvsd"] {
            if found.profiles.get(profile).is_some_and(|p| p.available) {
                offered.push(profile.to_owned());
            }
        }
        Ok(CardState {
            active: found.active_profile.clone(),
            headset_profiles: offered,
        })
    }

    fn set_profile(&mut self, card: &str, profile: &str) -> Result<(), String> {
        pactl(&["set-card-profile", card, profile])
            .map(|_| ())
            .ok_or_else(|| format!("could not switch the earbuds to {profile}"))
    }

    fn source_present(&mut self, device_id: &str) -> bool {
        listing().is_some_and(|listing| {
            listing.devices.iter().any(|device| {
                device.id == device_id
                    && device
                        .bluetooth
                        .as_ref()
                        .is_some_and(|card| card.source.is_some())
            })
        })
    }

    fn pause(&mut self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

/// The marker file: the card, then the profile to return it to.
struct MarkerFile;

impl MarkerFile {
    fn path() -> Option<std::path::PathBuf> {
        crate::app_paths::support_dir()
            .ok()
            .map(|dir| dir.join("bluetooth-profile.marker"))
    }
}

impl MarkerStore for MarkerFile {
    fn load(&mut self) -> Option<Marker> {
        let text = std::fs::read_to_string(Self::path()?).ok()?;
        let mut lines = text.lines();
        Some(Marker {
            card: lines.next()?.trim().to_owned(),
            previous: lines.next()?.trim().to_owned(),
        })
    }

    fn save(&mut self, marker: &Marker) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, format!("{}\n{}\n", marker.card, marker.previous))
        {
            tracing::warn!(%error, "could not record the earbuds' profile switch");
        }
    }

    fn clear(&mut self) {
        if let Some(path) = Self::path() {
            let _ = std::fs::remove_file(path);
        }
    }
}

enum Request {
    Begin { device_id: String },
    Restore(Option<Box<dyn FnOnce() + Send>>),
    Shutdown,
}

pub(crate) enum Event {
    /// The earbuds' microphone exists.
    Ready,
    Failed(String),
}

pub(crate) struct HeadsetFlow {
    requests: Sender<Request>,
    events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
    active: bool,
}

impl HeadsetFlow {
    /// Starts the worker, which first undoes a switch an earlier run left.
    pub(crate) fn start() -> Option<Self> {
        let (requests, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("bluetooth-headset".into())
            .spawn(move || serve(receiver, sender))
            .inspect_err(|error| tracing::warn!(%error, "could not start the earbuds flow"))
            .ok()?;
        Some(Self {
            requests,
            events,
            worker: Some(worker),
            active: false,
        })
    }

    /// Whether earbuds are (or are being put) in headset mode for a dictation.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// Switches the earbuds to headset mode; [`Self::poll`] reports when
    /// their microphone exists.
    pub(crate) fn begin(&mut self, device_id: &str) {
        self.active = true;
        let _ = self.requests.send(Request::Begin {
            device_id: device_id.to_owned(),
        });
    }

    pub(crate) fn poll(&mut self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Puts the earbuds back in music mode, then runs `then`.
    pub(crate) fn restore_then(&mut self, then: impl FnOnce() + Send + 'static) {
        self.active = false;
        if let Err(mpsc::SendError(Request::Restore(Some(then)))) =
            self.requests.send(Request::Restore(Some(Box::new(then))))
        {
            then();
        }
    }
}

impl Drop for HeadsetFlow {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(requests: Receiver<Request>, events: Sender<Event>) {
    let mut machine = Machine::new(Pactl, MarkerFile);
    machine.recover_stale();
    while let Ok(request) = requests.recv() {
        match request {
            Request::Begin { device_id } => {
                let _ = events.send(match begin(&mut machine, &device_id) {
                    Ok(()) => Event::Ready,
                    Err(error) => Event::Failed(error),
                });
            }
            Request::Restore(then) => {
                machine.restore();
                if let Some(then) = then {
                    then();
                }
            }
            Request::Shutdown => break,
        }
    }
    machine.restore();
}

fn begin(machine: &mut Machine<Pactl, MarkerFile>, device_id: &str) -> Result<(), String> {
    let listing = listing().ok_or("the sound server did not answer")?;
    let device = listing
        .devices
        .iter()
        .find(|device| device.id == device_id)
        .ok_or("the earbuds are not connected")?;
    let card = device
        .bluetooth
        .as_ref()
        .ok_or("that microphone is not Bluetooth")?;
    machine.begin(&card.card, device_id).map(|_| ())
}
