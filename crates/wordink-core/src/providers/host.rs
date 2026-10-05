//! Host-implemented providers (KTD5): the core delivers audio, the host
//! transcribes and reports back.

use super::{Capabilities, Ids, Progress, Run};
use crate::effects::{Effect, Event, HostResult};

/// A provider the host implements, such as `@wordink/local`.
///
/// A streaming host provider gets [`Effect::HostProviderStart`] when
/// listening starts and audio while the user speaks. A batch one gets
/// start, the whole utterance and finish on release.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HostProvider {
    /// Id the host registered the provider under, echoed on its effects.
    pub id: String,
    /// What the provider declared at registration.
    pub capabilities: Capabilities,
}

impl HostProvider {
    /// A host provider with the capabilities it declared.
    pub fn new(id: impl Into<String>, capabilities: Capabilities) -> Self {
        Self {
            id: id.into(),
            capabilities,
        }
    }
}

#[derive(Debug)]
pub(crate) struct HostRun {
    id: String,
    capabilities: Capabilities,
    hint: Option<String>,
    started: bool,
    done: bool,
}

impl HostRun {
    pub(crate) fn new(p: &HostProvider, hint: Option<&str>) -> Self {
        Self {
            id: p.id.clone(),
            capabilities: p.capabilities,
            hint: hint.map(str::to_owned),
            started: false,
            done: false,
        }
    }

    fn begin(&mut self, fx: &mut Vec<Effect>) {
        self.started = true;
        fx.push(Effect::HostProviderStart {
            id: self.id.clone(),
            sample_rate: self.capabilities.sample_rate,
            hint: self.hint.clone(),
        });
    }

    fn deliver(&self, samples: &[i16], fx: &mut Vec<Effect>) {
        if !samples.is_empty() {
            fx.push(Effect::HostProviderAudio {
                id: self.id.clone(),
                samples: samples.to_vec(),
            });
        }
    }
}

impl Run for HostRun {
    fn start(&mut self, _ids: &mut Ids, fx: &mut Vec<Effect>) {
        if self.capabilities.streaming {
            self.begin(fx);
        }
    }

    fn audio(&mut self, pcm: &[i16], fx: &mut Vec<Effect>) {
        self.deliver(pcm, fx);
    }

    fn finish(&mut self, all: &[i16], rest: &[i16], _ids: &mut Ids, fx: &mut Vec<Effect>) {
        if self.capabilities.streaming {
            self.deliver(rest, fx);
        } else {
            self.begin(fx);
            self.deliver(all, fx);
        }
        fx.push(Effect::HostProviderFinish {
            id: self.id.clone(),
        });
    }

    fn event(&mut self, event: Event, _ids: &mut Ids, _fx: &mut Vec<Effect>) -> Option<Progress> {
        let Event::HostProviderResult(result) = event else {
            return None;
        };
        if !self.started || self.done {
            return None;
        }
        Some(match result {
            HostResult::Interim { text } => Progress::Interim(text),
            HostResult::Final { text } => {
                self.done = true;
                Progress::Final(text)
            }
            HostResult::Error { code } => {
                self.done = true;
                Progress::Failed(code)
            }
        })
    }

    fn end(&mut self, fx: &mut Vec<Effect>) {
        if self.started && !self.done {
            self.done = true;
            fx.push(Effect::HostProviderCancel {
                id: self.id.clone(),
            });
        }
    }
}
