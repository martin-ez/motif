//! What a stream plays, and the one choice that used to be the only one.
//!
//! A stream captures frames and plays frames; between the two is a decision
//! only a caller can make. [`AudioPath`] is where that decision goes: built on
//! the thread that opens the stream, called on the thread that may not
//! allocate, and the way anything a player is meant to hear — a loop, a
//! metronome click, a monitored input — reaches the audio callback at all.
//!
//! [`Passthrough`] plays the frames it captured and [`InputMonitor`] is the same
//! thing with a level on it, while [`Opening`] is the level a stream comes up to
//! when it opens. A queue has one reader, and [`Commanded`] is it: it deals what
//! arrived to the path it holds, which may be a composition of several. Which of
//! them a command belongs to is that composition's to say in
//! [`apply`](AudioPath::apply), settled where the paths are put together.

use super::{Command, CommandReceiver, Gain, StreamConfig};

const UNITY: f32 = 1.0;

/// The multiplier a stream opens at where nobody has chosen its devices.
///
/// Twelve decibels below unity, which is exactly the range the panel's encoder
/// has above it: unity is still reachable, but only by asking for the whole of
/// that range. A run nobody has pointed at an interface is playing into
/// whatever the machine offered, and a built-in microphone in front of built-in
/// speakers is a feedback loop that unity is where it runs away from.
pub const GUARDED_LEVEL: f32 = UNITY / Gain::CEILING;

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

    /// Take `command` if it was meant for this path, and say whether it was.
    ///
    /// Answering one is the end of it: a composition offers a command along the
    /// paths it holds until one takes it, so exactly one applies it and no
    /// second reader is left to go without. A path answering nothing says so on
    /// every command, and there is no default — what a path answers is part of
    /// what it is.
    ///
    /// Runs on the audio thread, under [`render`](Self::render)'s rules.
    fn apply(&mut self, command: Command) -> bool;
}

/// A path that may not be there, and silence where it is not.
///
/// A [`DeviceLink`](super::DeviceLink) builds a path per stream it opens, which
/// a path holding one end of something cannot answer twice: a loop engine holds
/// the receiving end of the command queue and the publishing end of the
/// playhead, and there is one of each. [`Escrow`](super::Escrow) is what lends
/// such a path from one stream to the next, and this is what it plays meanwhile.
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

    fn apply(&mut self, command: Command) -> bool {
        match self {
            Some(path) => path.apply(command),
            None => false,
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

    /// Nothing to answer: what it plays is what it was handed, and there is no
    /// command that changes that.
    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// The path that plays the input at a level the player controls.
///
/// [`Passthrough`] with a hand on it: the same frames, scaled by a [`Gain`] the
/// player moves and mutes from the panel. Those two commands are the whole of
/// what it answers, so a composition holding it and something else is free to
/// give the rest away.
///
/// It reads no queue of its own. [`Commanded`] is what puts one in front of it.
pub struct InputMonitor {
    gain: Gain,
}

impl InputMonitor {
    /// A monitor at unity.
    pub const fn new() -> Self {
        Self {
            gain: Gain::unity(),
        }
    }

    /// The gain the input is being played at.
    pub const fn gain(&self) -> &Gain {
        &self.gain
    }
}

impl Default for InputMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPath for InputMonitor {
    /// Puts the ramp in the frames the device granted, so a change takes the
    /// same time whatever rate it was opened at.
    fn prepare(&mut self, config: StreamConfig) {
        self.gain.prepare(config.sample_rate);
    }

    /// Plays the captured frames at the gain the player left it on. Bounded by
    /// the shorter of the two slices, as [`Passthrough`] is, and allocating
    /// nothing.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        let frames = playing.len().min(captured.len());
        playing[..frames].copy_from_slice(&captured[..frames]);
        self.gain.apply(&mut playing[..frames]);
    }

    /// Answers what moves the level the input is played at, and nothing about
    /// the loop under it.
    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::SetGain(gain) => self.gain.set_target(gain),
            Command::SetMuted(muted) => self.gain.set_muted(muted),
            Command::SetTransport(_) | Command::Undo | Command::Clear => return false,
        }

        true
    }
}

/// A path with the level its stream opens at in front of it.
///
/// Two things a stream owes the room it plays into. It comes up from silence
/// over [`Gain::RAMP`] rather than landing on its level in the first block,
/// which a device reopening after a fault owes as much as one opening for the
/// first time. And it opens at whatever level it was given, which is
/// [`GUARDED_LEVEL`] where nothing has said what it is playing into.
///
/// The trim is on what is played, so it covers a loop as much as a monitored
/// input, and no command moves it.
///
/// ```
/// use motif::audio::{AudioPath, Opening, Passthrough, StreamConfig};
///
/// let mut path = Opening::at(1.0, Passthrough::new());
/// path.prepare(StreamConfig {
///     sample_rate: 48_000,
///     block_size: 4,
///     input_channels: 1,
///     output_channels: 1,
/// });
///
/// let mut playing = [0.0; 4];
/// path.render(&[1.0; 4], &mut playing);
///
/// assert_eq!(playing[0], 0.0);
/// assert!(playing[3] > playing[0]);
/// ```
pub struct Opening<P> {
    gain: Gain,
    level: f32,
    path: P,
}

impl<P: AudioPath> Opening<P> {
    /// `path`, opening at `level`.
    pub const fn at(level: f32, path: P) -> Self {
        Self {
            gain: Gain::rising(),
            level,
            path,
        }
    }
}

impl<P: AudioPath> AudioPath for Opening<P> {
    /// Puts the gain back at silence, so that every stream opened on this path
    /// comes up rather than only the first.
    fn prepare(&mut self, config: StreamConfig) {
        self.path.prepare(config);
        self.gain = Gain::rising();
        self.gain.set_target(self.level);
        self.gain.prepare(config.sample_rate);
    }

    /// Trims the whole of what the path under it played, allocating nothing and
    /// costing the multiply and add [`Gain::apply`] costs.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.path.render(captured, playing);
        self.gain.apply(playing);
    }

    /// Answers whatever the path it holds answers, having nothing of its own to
    /// take: the level a stream opens at is not one the player moves.
    fn apply(&mut self, command: Command) -> bool {
        self.path.apply(command)
    }
}

/// A path with the command queue in front of it.
///
/// The one reader of that queue: everything waiting is dealt to `path` before
/// the block it arrived in is rendered, and a command nothing answers is
/// discarded there rather than left to accumulate.
///
/// `path` is a composition where more than one thing takes orders, which is
/// what keeps a queue to a single reader however many paths are behind it.
///
/// ```
/// use motif::audio::{AudioPath, Command, Commanded, InputMonitor, SendError, command_channel};
///
/// let (mut player, commands) = command_channel(4);
/// let mut path = Commanded::new(commands, InputMonitor::new());
///
/// player.send(Command::SetGain(0.5))?;
/// path.render(&[1.0], &mut [0.0]);
///
/// assert_eq!(path.path().gain().target(), 0.5);
/// # Ok::<(), SendError>(())
/// ```
pub struct Commanded<P> {
    commands: CommandReceiver,
    path: P,
}

impl<P: AudioPath> Commanded<P> {
    /// `path`, taking what the player asks for from `commands`.
    pub const fn new(commands: CommandReceiver, path: P) -> Self {
        Self { commands, path }
    }

    /// The path the commands are dealt to.
    pub const fn path(&self) -> &P {
        &self.path
    }
}

impl<P: AudioPath> AudioPath for Commanded<P> {
    /// Prepares the path it holds; a queue is allocated before either of them
    /// exists.
    fn prepare(&mut self, config: StreamConfig) {
        self.path.prepare(config);
    }

    /// Deals every command that was waiting when the block began, then renders
    /// the path. The count is fixed before the loop starts, so a sender running
    /// concurrently cannot lengthen it.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        for command in self.commands.drain() {
            self.path.apply(command);
        }

        self.path.render(captured, playing);
    }

    /// Answers whatever the path it holds answers, so a commanded path composes
    /// inside another.
    fn apply(&mut self, command: Command) -> bool {
        self.path.apply(command)
    }
}
