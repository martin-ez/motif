//! The last listing a backend gave, held so that drawing it again costs
//! nothing.
//!
//! On ALSA, enumeration is `snd_pcm_open` on every input-capable and every
//! output-capable PCM. It is not transactional on failure — a plugin failing
//! with `EPERM` leaks the descriptor — so a page enumerating each time it opens
//! eventually poisons the backend, and one enumerating while a stream runs
//! takes `EBUSY` on the PCM that stream holds and drops the device the player
//! is hearing.
//!
//! [`DeviceCatalog`] sits in front of an [`AudioBackend`] rather than behind
//! it, so `hosts` stays the live query its own documentation says it is.

use super::{AudioBackend, AudioDevice, AudioHost, DeviceSelection};

/// A listing, and the choice of when to pay for another one.
///
/// Reading costs nothing however often a page is drawn; only
/// [`refresh`](Self::refresh) reaches the backend. Nothing here runs on the
/// audio thread — refreshing blocks and allocates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCatalog {
    sample_rate: u32,
    hosts: Vec<AudioHost>,
    stale: bool,
}

impl DeviceCatalog {
    /// A catalog that will list what a backend offers at `sample_rate`, having
    /// asked nothing yet.
    ///
    /// Touches no device, so the first [`refresh`](Self::refresh) is where
    /// enumeration is first paid for.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            hosts: Vec::new(),
            stale: true,
        }
    }

    /// The listing as of the last [`refresh`](Self::refresh), which nothing
    /// promises is still true.
    ///
    /// Empty before the first refresh, and [`is_stale`](Self::is_stale) is what
    /// tells that apart from a machine with nothing on it.
    pub fn hosts(&self) -> &[AudioHost] {
        &self.hosts
    }

    /// Whether the listing predates any refresh, and so describes nothing that
    /// was ever asked.
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// Enumerate again, keeping whatever `held` names that the new listing
    /// lost.
    ///
    /// `held` is the selection a stream is open on, and is the whole scope of
    /// what a refresh may not take away: enumerating opens the PCM that stream
    /// holds and takes `EBUSY`, so the device being heard is exactly the row
    /// that disappears. A carried row is appended, keeping the channel counts
    /// it was last listed with. Pass `None` where nothing is open, so that a
    /// device which has genuinely gone can go.
    ///
    /// Blocks and allocates; never reach it from the audio callback.
    pub fn refresh(&mut self, backend: &impl AudioBackend, held: Option<&DeviceSelection>) {
        let listed = backend.hosts(self.sample_rate);
        let previous = std::mem::replace(&mut self.hosts, listed);
        self.stale = false;

        if let Some(held) = held {
            self.carry(&previous, held);
        }
    }

    fn carry(&mut self, previous: &[AudioHost], held: &DeviceSelection) {
        let Some(before) = previous.iter().find(|host| host.name == held.host) else {
            return;
        };
        let input = lost(&before.inputs, &held.input, self.listed_inputs(&held.host));
        let output = lost(
            &before.outputs,
            &held.output,
            self.listed_outputs(&held.host),
        );

        if input.is_none() && output.is_none() {
            return;
        }
        if !self.hosts.iter().any(|host| host.name == held.host) {
            self.hosts.push(AudioHost {
                name: held.host.clone(),
                inputs: Vec::new(),
                outputs: Vec::new(),
            });
        }

        let Some(host) = self.hosts.iter_mut().find(|host| host.name == held.host) else {
            return;
        };
        host.inputs.extend(input.cloned());
        host.outputs.extend(output.cloned());
    }

    fn listed_inputs(&self, host: &str) -> &[AudioDevice] {
        self.devices(host, |host| &host.inputs)
    }

    fn listed_outputs(&self, host: &str) -> &[AudioDevice] {
        self.devices(host, |host| &host.outputs)
    }

    fn devices(&self, host: &str, of: fn(&AudioHost) -> &Vec<AudioDevice>) -> &[AudioDevice] {
        self.hosts
            .iter()
            .find(|listed| listed.name == host)
            .map_or(&[][..], |listed| of(listed))
    }
}

fn lost<'a>(before: &'a [AudioDevice], name: &str, now: &[AudioDevice]) -> Option<&'a AudioDevice> {
    if now.iter().any(|device| device.name == name) {
        return None;
    }
    before.iter().find(|device| device.name == name)
}
