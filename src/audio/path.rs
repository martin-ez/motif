//! What a stream plays, and the one choice that used to be the only one.
//!
//! A stream captures frames and plays frames; between the two is a decision
//! only a caller can make. [`AudioPath`] is where that decision goes: built on
//! the thread that opens the stream, called on the thread that may not
//! allocate, and the way anything a player is meant to hear — a loop, a
//! metronome click, a monitored input — reaches the audio callback at all.
//!
//! [`Passthrough`] is what every stream once did and now merely may: play the
//! frames it captured. [`InputMonitor`] is the same thing with a level on it,
//! and the first path that takes anything from the player — which it does over
//! a command queue, that being the only way onto the audio thread.

use super::{Command, CommandReceiver, Gain, StreamConfig};

/// What a stream plays, block by block.
///
/// The two halves sit either side of invariant 2:
/// [`prepare`](Self::prepare) runs where allocating is allowed and
/// [`render`](Self::render) where it is not.
pub trait AudioPath: Send + 'static {
    /// Prepare to run at `config`, before any block arrives.
    ///
    /// Called once, on the thread that opened the stream, and the only place a
    /// path may allocate.
    ///
    /// `config` is what the stream was opened for, which is not always what it
    /// goes on to report: a device may grant a shorter block, having taken the
    /// path already. Size buffers from `block_size` and read it as a ceiling on
    /// what [`render`](Self::render) is handed, never as a promise.
    fn prepare(&mut self, config: StreamConfig);

    /// Play into `playing`, given the `captured` frames that arrived with it.
    ///
    /// One sample per frame in both directions and the same count in each: a
    /// channel layout belongs to the device, not here. `playing` arrives
    /// silent, so a path with nothing to play may leave it alone.
    ///
    /// Runs on the audio thread, so it may not allocate, lock or panic. Neither
    /// slice is bounded by what [`prepare`](Self::prepare) was told, so bound
    /// the work by the slices that were handed over.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]);
}

/// A path a caller has one of, and silence once it has been handed over.
///
/// A [`DeviceLink`](super::DeviceLink) builds a path per stream it opens, which
/// a path holding one end of something cannot answer twice: a loop engine holds
/// the receiving end of the command queue and the publishing end of the
/// playhead, and there is one of each. `move || path.take()` is how such a path
/// reaches the first stream, and `None` is what a stream opened after it plays.
impl<P: AudioPath> AudioPath for Option<P> {
    fn prepare(&mut self, config: StreamConfig) {
        if let Some(path) = self {
            path.prepare(config);
        }
    }

    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        if let Some(path) = self {
            path.render(captured, playing);
        }
    }
}

/// The path that plays the frames it captured.
///
/// What a stream did before anything could say otherwise, and what a caller
/// chooses to go on monitoring the input.
pub struct Passthrough;

impl Passthrough {
    /// A path that plays what it hears.
    pub const fn new() -> Self {
        Self
    }
}

impl Default for Passthrough {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPath for Passthrough {
    /// Nothing to prepare: what it plays is what it was handed, at whatever
    /// rate and block size that arrives in.
    fn prepare(&mut self, _config: StreamConfig) {}

    /// Copies as many frames as both slices have and leaves the rest of
    /// `playing` as it found it, a stream having silenced the block already: a
    /// panic on the audio thread is the worse answer to a mismatch.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        let frames = playing.len().min(captured.len());
        playing[..frames].copy_from_slice(&captured[..frames]);
    }
}

/// The path that plays the input at a level the player controls.
///
/// [`Passthrough`] with a hand on it: the same frames, scaled by a [`Gain`] the
/// player moves and mutes from the panel. It takes those changes off a command
/// queue, which is how anything reaches the audio thread.
///
/// It drains the queue it is given, so a composition hands it the receiving end
/// of a queue it is the only reader of. Commands it does not answer are taken
/// and discarded, not left for someone else.
pub struct InputMonitor {
    commands: CommandReceiver,
    gain: Gain,
}

impl InputMonitor {
    /// A monitor at unity, taking its changes from `commands`.
    pub const fn new(commands: CommandReceiver) -> Self {
        Self {
            commands,
            gain: Gain::unity(),
        }
    }

    /// The gain the input is being played at.
    pub const fn gain(&self) -> &Gain {
        &self.gain
    }

    fn take_the_commands_that_arrived(&mut self) {
        for command in self.commands.drain() {
            match command {
                Command::SetGain(gain) => self.gain.set_target(gain),
                Command::SetMuted(muted) => self.gain.set_muted(muted),
                Command::SetTransport(_) | Command::Undo | Command::Clear => {}
            }
        }
    }
}

impl AudioPath for InputMonitor {
    /// Puts the ramp in the frames the device granted, so a change takes the
    /// same time whatever rate it was opened at.
    fn prepare(&mut self, config: StreamConfig) {
        self.gain.prepare(config.sample_rate);
    }

    /// Takes every command that was waiting, then plays the captured frames at
    /// the gain they left behind. Bounded by the shorter of the two slices, as
    /// [`Passthrough`] is, and allocating nothing.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.take_the_commands_that_arrived();

        let frames = playing.len().min(captured.len());
        playing[..frames].copy_from_slice(&captured[..frames]);
        self.gain.apply(&mut playing[..frames]);
    }
}
