//! Opening a duplex audio stream, the boundary between its callback and the
//! rest of the system, and the [`AudioPath`] a caller runs on it.
//!
//! Devices are reached through [`AudioBackend`], so one with no hardware behind
//! it can stand in where none exists, and [`DeviceCatalog`] caches a listing.
//!
//! Everything crossing the callback boundary does so over a lock-free channel
//! built here: [`sample_ring`] for the audio, [`command_channel`] for changes
//! going the other way, [`level_meter`], [`headroom_meter`] and [`sample_clock`]
//! for how loud it was, how close the deadline came and how many frames have
//! gone by, [`priority_latch`] for the class the callback runs at, and
//! [`xrun_counter`] and [`fault_channel`] for the two ways the boundary fails.
//! A fault outlives the stream it came from, so [`DeviceLink`] holds what it
//! takes to open another and is what the rest of the application talks to.

use std::fmt;

mod boundary;
mod catalog;
mod clock;
mod command;
mod cpal_backend;
mod fault;
mod gain;
mod headroom;
mod level;
mod link;
mod path;
mod placement;
mod ring;
mod xrun;

pub use boundary::{BlockCapture, BlockPlayback, boundary};
pub use catalog::DeviceCatalog;
pub use clock::{Counting, SampleClockReader, SampleClockWriter, sample_clock};
pub use command::{Command, CommandReceiver, CommandSender, SendError, command_channel};
pub use cpal_backend::{CpalBackend, CpalStream};
pub use fault::{FaultReader, FaultReporter, fault_channel};
pub use gain::Gain;
pub use headroom::{Headroom, HeadroomReader, HeadroomWriter, headroom_meter};
pub use level::{LevelReader, LevelWriter, Levels, level_meter};
pub use link::{AudioState, DeviceLink, SharedLink};
pub use path::{AudioPath, Commanded, InputMonitor, Passthrough};
pub use placement::{
    Grant, HOSTED_PRIORITY, Placed, Placement, PriorityReader, PriorityReporter, pinning,
    priority_latch,
};
pub use ring::{SampleConsumer, SampleProducer, sample_ring};
pub use xrun::{OverrunCounter, UnderrunCounter, XrunReader, Xruns, xrun_counter};

/// The sample rate and block size a stream is asked to run at.
///
/// A device is free to grant something else; read [`DuplexStream::config`] for
/// what it actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamRequest {
    /// Frames per second.
    pub sample_rate: u32,
    /// Frames per callback.
    pub block_size: u32,
}

/// The configuration a device is running at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    /// Frames per second.
    pub sample_rate: u32,
    /// Frames per callback.
    ///
    /// Where the input and output streams negotiate separately, this is the
    /// output stream's, and the input's may differ. It is also not a bound a
    /// caller may rely on: size buffers from it, but bound writes by the length
    /// of the slice actually handed to the callback.
    pub block_size: u32,
    /// Channels the input stream delivers.
    pub input_channels: u16,
    /// Channels the output stream consumes.
    pub output_channels: u16,
}

/// An audio API, and the devices reachable through it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioHost {
    /// What the backend calls this host.
    pub name: String,
    /// Devices that can deliver input.
    pub inputs: Vec<AudioDevice>,
    /// Devices that can consume output.
    pub outputs: Vec<AudioDevice>,
}

/// Which device a listing meant, where a name is not enough to say.
///
/// Two interfaces of one model describe themselves identically, and on ALSA the
/// `hw:` and `plughw:` entries for one card share a first description line. The
/// name is what a player reads; this is what selects between the rows carrying
/// it, and no two devices in one direction of a host share one.
///
/// ```
/// use motif::audio::DeviceId;
///
/// let first = DeviceId::named("interface");
/// let second = DeviceId { nth: 1, ..first.clone() };
///
/// assert_ne!(first, second);
/// assert_eq!(second.to_string(), "interface (2)");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceId {
    /// What the host calls the device.
    pub name: String,
    /// Which device of this name, counted from zero in the order the host
    /// enumerates them.
    pub nth: usize,
}

impl DeviceId {
    /// The first device of a name, which is the whole of it where a name is
    /// unique.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            nth: 0,
        }
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.nth {
            0 => f.write_str(&self.name),
            nth => write!(f, "{} ({})", self.name, nth + 1),
        }
    }
}

/// A device, and the channel counts it can be opened with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    /// Which device this is, name included.
    pub id: DeviceId,
    /// Channel counts the device can be opened with, ascending and without
    /// repeats.
    pub channels: Vec<u16>,
}

/// Which of a device's channels carry audio.
///
/// A run of adjacent channels rather than an arbitrary set: an instrument is
/// patched into neighbouring inputs of an interface, and a set would put a
/// per-channel branch in the callback for a case nobody is asking for.
///
/// Which channels rather than how many, because folding a whole frame together
/// costs level: a source wired to one input of a stereo pair arrives 6 dB down
/// in the mean of the two. Input gain is the answer where a player has no
/// choice; naming the channel is the better one where they have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSelection {
    /// The first channel used, counted from zero.
    pub first: u16,
    /// How many consecutive channels from [`first`](Self::first) are used.
    pub count: u16,
}

impl ChannelSelection {
    /// Every channel of a device that is `channels` wide.
    pub const fn all(channels: u16) -> Self {
        Self {
            first: 0,
            count: channels,
        }
    }

    /// The narrowest a device can be opened and still have these channels:
    /// inputs three and four cannot be reached without opening four.
    ///
    /// Wider than a channel count, because the sum of two `u16` is not one. A
    /// selection past the end of `u16` has to come back as a reach no device
    /// meets; saturating it would land on `u16::MAX`, which a device can meet
    /// and which then describes a run of no channels at all.
    pub const fn reach(self) -> u32 {
        self.first as u32 + self.count as u32
    }
}

/// Which devices to open, and which of their channels carry audio.
///
/// The host is [`AudioHost::name`] and the devices are [`AudioDevice::id`] as
/// [`AudioBackend::hosts`] gives them, so a selection is built straight out of a
/// listing and nothing has to recover from a string what the listing knew.
///
/// A listing was true when it was drawn and nothing promises it still is, so a
/// selection naming a device that has since gone is the ordinary case rather
/// than a misuse: [`AudioBackend::open`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSelection {
    /// The host both devices are reached through.
    pub host: String,
    /// The device audio is captured from.
    pub input: DeviceId,
    /// Which of the input device's channels are captured.
    pub input_channels: ChannelSelection,
    /// The device audio is played to.
    pub output: DeviceId,
    /// Which of the output device's channels are played to.
    pub output_channels: ChannelSelection,
}

impl DeviceSelection {
    /// A selection naming no host and no device.
    ///
    /// What a run starts from where the backend offered no default. Opening it
    /// fails the way a device unplugged since the listing does, which is a state
    /// the screen already draws — so a machine with nothing to open still
    /// reaches the page that picks a device, instead of losing it exactly when
    /// it is needed.
    pub fn nothing() -> Self {
        Self {
            host: String::new(),
            input: DeviceId::named(""),
            input_channels: ChannelSelection::all(0),
            output: DeviceId::named(""),
            output_channels: ChannelSelection::all(0),
        }
    }
}

/// Whether a stream is currently calling back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// No callback is running.
    Stopped,
    /// A callback may be running.
    ///
    /// The weaker reading is the useful one: a caller may not touch a buffer
    /// the callback owns while a stream reports this, and a stream that failed
    /// partway through stopping reports it too.
    Running,
}

/// Why a device could not be opened, started or stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceError {
    /// The backend has no host by the name that was selected.
    NoSuchHost,
    /// The host has no input device the selection identifies.
    NoInputDevice,
    /// The host has no output device the selection identifies.
    NoOutputDevice,
    /// The device cannot run at the requested configuration.
    UnsupportedConfig,
    /// The device is not available, having been disconnected or claimed
    /// elsewhere.
    DeviceNotAvailable,
    /// The operating system refused access to the device, as it does for
    /// microphone input until the user grants permission.
    PermissionDenied,
    /// The backend failed for a reason it did not classify.
    BackendFailure,
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let described = match self {
            Self::NoSuchHost => "there is no host by that name",
            Self::NoInputDevice => "the host has no such input device",
            Self::NoOutputDevice => "the host has no such output device",
            Self::UnsupportedConfig => "the device cannot run at the requested configuration",
            Self::DeviceNotAvailable => "the device is not available",
            Self::PermissionDenied => "access to the device was refused",
            Self::BackendFailure => "the audio backend failed",
        };
        f.write_str(described)
    }
}

impl std::error::Error for DeviceError {}

/// A source of duplex audio streams, a description of what there is to open,
/// and what it would open if nobody chose.
pub trait AudioBackend {
    /// The stream this backend opens.
    type Stream: DuplexStream;

    /// The hosts and devices there are to open at `sample_rate`.
    ///
    /// Listed means openable: a device that comes back offers `f32` at
    /// `sample_rate` on every channel count it lists, ascending and without
    /// repeats. One that cannot is absent rather than listed and unopenable, as
    /// is a host left with nothing behind it, or one that will not answer.
    ///
    /// Only that direction is promised — a backend may open something it did not
    /// list, as [`NullBackend::rounding`] does — so this is a menu, not a ruling.
    ///
    /// Blocks and allocates; never reach it from the audio callback.
    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost>;

    /// What the backend would open at `sample_rate` if nobody chose, or `None`
    /// where it has no device in one of the two directions.
    ///
    /// The host's own defaults rather than the first row of
    /// [`hosts`](Self::hosts), an operating system's default being the better
    /// guess — and a guess is what this is, a starting point for something not
    /// yet chosen rather than a promise [`open`](Self::open) will meet it.
    ///
    /// Blocks and allocates; never reach it from the audio callback.
    fn defaults(&self, sample_rate: u32) -> Option<DeviceSelection>;

    /// Open an input and an output stream on `selection` at `request`, playing
    /// what `path` plays and leaving other channels opened but untouched.
    ///
    /// Each is opened at the narrowest count reaching both the selection and the
    /// width it runs at by default — in mono, a stereo device folds a pair.
    /// `path` is prepared with what the stream was opened for, then moved in.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError`] for a host or device that is not there, an
    /// unreachable selection, or a `request` it cannot run. Never a panic.
    fn open<P: AudioPath>(
        &self,
        selection: &DeviceSelection,
        request: StreamRequest,
        path: P,
    ) -> Result<Self::Stream, DeviceError>;
}

fn opened_width(offered: &[u16], selection: ChannelSelection, natural: u16) -> Option<u16> {
    if selection.count == 0 {
        return None;
    }

    let narrowest_reaching = |needed: u32| {
        offered
            .iter()
            .copied()
            .find(move |&width| u32::from(width) >= needed)
    };

    narrowest_reaching(selection.reach().max(u32::from(natural)))
        .or_else(|| narrowest_reaching(selection.reach()))
}

/// An input and an output stream running together on one device.
pub trait DuplexStream {
    /// The configuration the device granted, which may differ from the request.
    fn config(&self) -> StreamConfig;

    /// Whether the callback is running.
    fn state(&self) -> StreamState;

    /// How loud the most recent block of input was, over the channels the
    /// stream captures and no others.
    ///
    /// Measured in the callback and published without a lock, so this reads
    /// whatever the last block to arrive measured, and reads it again until the
    /// next one does. A stopped stream keeps reporting the block it stopped on.
    fn levels(&self) -> Levels;

    /// How many callbacks have lost frames in each direction.
    ///
    /// Counted from when the stream was opened, and never cleared — stopping
    /// and starting one does not reset them.
    fn xruns(&self) -> Xruns;

    /// How much of its deadline the callback used, over the recent window.
    ///
    /// A stream with a callback in each direction reports the tighter of the
    /// two, since either one missing its deadline is the stream missing it. The
    /// window advances with the blocks that arrive rather than with the clock,
    /// so a stopped stream keeps reporting the window it stopped in.
    fn headroom(&self) -> Headroom;

    /// Where the callback thread ended up, and what the host would not do.
    ///
    /// Asked once, on the first block a callback runs, so a stream that has run
    /// none reports [`Placed::UNASKED`]. A host that placed nothing is a stream
    /// that runs anyway: a placement is an advantage the callback takes where
    /// it is offered, not something it needs to work.
    fn placement(&self) -> Placed;

    /// Why the device failed, or `None` while it has not.
    ///
    /// This is the fault a callback reported, latched and read here rather than
    /// returned from anything: a device goes away between calls, not during
    /// one, so there is no method for it to fail. The first fault is the one
    /// kept, and it is kept for the life of the stream — a stream that has
    /// faulted is finished, and [`DeviceLink`] is what opens another.
    fn fault(&self) -> Option<DeviceError>;

    /// Start calling back.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError`] when the device refuses to start.
    fn start(&mut self) -> Result<(), DeviceError>;

    /// Stop calling back.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError`] when the device refuses to stop.
    fn stop(&mut self) -> Result<(), DeviceError>;
}

/// A backend with no audio hardware behind it.
///
/// It exists so that the stream lifecycle can be exercised where no device is
/// present, which is the case on continuous integration runners.
pub struct NullBackend {
    granted: StreamConfig,
    rejects_mismatch: bool,
    widths: Option<Vec<u16>>,
    twin: Option<Vec<u16>>,
}

impl NullBackend {
    /// A backend whose device grants its own configuration whatever is asked
    /// of it, modelling a device that rounds a request to what it supports.
    pub fn rounding(granted: StreamConfig) -> Self {
        Self {
            granted,
            rejects_mismatch: false,
            widths: None,
            twin: None,
        }
    }

    /// A backend whose device refuses any request it cannot meet exactly,
    /// modelling a device that supports one configuration and no other.
    pub fn rejecting(granted: StreamConfig) -> Self {
        Self {
            granted,
            rejects_mismatch: true,
            widths: None,
            twin: None,
        }
    }

    /// A rounding backend whose device in each direction is followed by a twin:
    /// a second device of the same name, opening at any of `widths`.
    ///
    /// Two devices describing themselves identically is the case a name alone
    /// cannot select between, and modelling it takes no hardware. The twin
    /// grants a width of its own so that which of the pair was opened is
    /// visible in a [`DuplexStream::config`].
    pub fn twinned(granted: StreamConfig, widths: Vec<u16>) -> Self {
        let mut widths = widths;
        widths.sort_unstable();
        widths.dedup();

        Self {
            granted,
            rejects_mismatch: false,
            widths: None,
            twin: Some(widths),
        }
    }

    /// A rounding backend whose device opens at any of `widths` in either
    /// direction, sorted and deduplicated as a listing is.
    ///
    /// Which width [`AudioBackend::open`] picks is only observable against a
    /// device offering more than one: where there is a single width, that rule
    /// and its opposite both open it. `granted` still names the width the
    /// device runs at, so both halves of the rule are reachable here.
    pub fn offering(granted: StreamConfig, widths: Vec<u16>) -> Self {
        let mut widths = widths;
        widths.sort_unstable();
        widths.dedup();

        Self {
            granted,
            rejects_mismatch: false,
            widths: Some(widths),
            twin: None,
        }
    }

    fn offered(&self, natural: u16) -> Vec<u16> {
        if natural == 0 {
            return Vec::new();
        }
        match &self.widths {
            Some(widths) => widths.clone(),
            None => vec![natural],
        }
    }

    fn null_devices(&self, name: &str, natural: u16) -> Vec<AudioDevice> {
        let offered = self.offered(natural);
        if offered.is_empty() {
            return Vec::new();
        }

        std::iter::once(offered)
            .chain(self.twin.clone())
            .filter(|channels| !channels.is_empty())
            .enumerate()
            .map(|(nth, channels)| AudioDevice {
                id: DeviceId {
                    name: name.to_owned(),
                    nth,
                },
                channels,
            })
            .collect()
    }
}

const NULL_HOST: &str = "null";
const NULL_INPUT: &str = "null input";
const NULL_OUTPUT: &str = "null output";

impl AudioBackend for NullBackend {
    type Stream = NullStream;

    /// One host carrying one device in each direction the granted
    /// configuration has channels for, listed only at the rate it was
    /// granted — so that a caller relying on "listed means openable" can be
    /// tested where there is no hardware.
    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost> {
        if sample_rate != self.granted.sample_rate {
            return Vec::new();
        }

        let host = AudioHost {
            name: NULL_HOST.to_owned(),
            inputs: self.null_devices(NULL_INPUT, self.granted.input_channels),
            outputs: self.null_devices(NULL_OUTPUT, self.granted.output_channels),
        };

        if host.inputs.is_empty() && host.outputs.is_empty() {
            return Vec::new();
        }
        vec![host]
    }

    /// The one device it has in each direction, across the width it grants
    /// them at, at the rate it lists them at.
    fn defaults(&self, sample_rate: u32) -> Option<DeviceSelection> {
        let host = self.hosts(sample_rate).into_iter().next()?;
        let input = host.inputs.first()?;
        let output = host.outputs.first()?;

        Some(DeviceSelection {
            input: input.id.clone(),
            input_channels: ChannelSelection::all(self.granted.input_channels),
            output: output.id.clone(),
            output_channels: ChannelSelection::all(self.granted.output_channels),
            host: host.name,
        })
    }

    /// Devices are looked up whatever the rate asked for, so that a rounding
    /// device can still be asked for a rate it does not list and grant its own.
    fn open<P: AudioPath>(
        &self,
        selection: &DeviceSelection,
        request: StreamRequest,
        mut path: P,
    ) -> Result<Self::Stream, DeviceError> {
        if selection.host != NULL_HOST {
            return Err(DeviceError::NoSuchHost);
        }

        let inputs = self.null_devices(NULL_INPUT, self.granted.input_channels);
        let input = inputs
            .iter()
            .find(|device| device.id == selection.input)
            .ok_or(DeviceError::NoInputDevice)?;
        let outputs = self.null_devices(NULL_OUTPUT, self.granted.output_channels);
        let output = outputs
            .iter()
            .find(|device| device.id == selection.output)
            .ok_or(DeviceError::NoOutputDevice)?;

        let input_channels = opened_width(
            &input.channels,
            selection.input_channels,
            self.granted.input_channels,
        )
        .ok_or(DeviceError::UnsupportedConfig)?;
        let output_channels = opened_width(
            &output.channels,
            selection.output_channels,
            self.granted.output_channels,
        )
        .ok_or(DeviceError::UnsupportedConfig)?;

        let matches_exactly = request.sample_rate == self.granted.sample_rate
            && request.block_size == self.granted.block_size;
        if self.rejects_mismatch && !matches_exactly {
            return Err(DeviceError::UnsupportedConfig);
        }

        let (reporter, faults) = fault_channel();
        let config = StreamConfig {
            input_channels,
            output_channels,
            ..self.granted
        };
        path.prepare(config);

        Ok(NullStream {
            config,
            state: StreamState::Stopped,
            reporter,
            faults,
            path: Box::new(path),
        })
    }
}

/// A stream with no device behind it, opened by [`NullBackend`].
pub struct NullStream {
    config: StreamConfig,
    state: StreamState,
    reporter: FaultReporter,
    faults: FaultReader,
    path: Box<dyn AudioPath>,
}

impl NullStream {
    /// Report `error` against this stream, as a real device's error callback
    /// would.
    ///
    /// A device cannot be unplugged from a test, so this stands in for the
    /// unplugging. It is public for the same reason [`NullBackend`] is: the
    /// recovery path is the part of device loss that has to work, and it would
    /// otherwise be exercisable only where there is hardware to pull out.
    ///
    /// Takes `&self`, because that is what an error callback has.
    pub fn fail(&self, error: DeviceError) {
        self.reporter.report(error);
    }

    /// Hand `captured` to the path this stream was opened with, and fill
    /// `playing` with what it plays.
    ///
    /// A stream with no device behind it is never called back, so this stands in
    /// for the callback as [`fail`](Self::fail) stands in for the unplugging,
    /// and promises the path what a callback does: `playing` silenced first, and
    /// a frame of it for every frame handed over, so two lengths become the
    /// shorter of them rather than a broken contract.
    pub fn block(&mut self, captured: &[f32], playing: &mut [f32]) {
        playing.fill(0.0);

        let frames = captured.len().min(playing.len());
        self.path
            .render(&captured[..frames], &mut playing[..frames]);
    }
}

impl DuplexStream for NullStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn state(&self) -> StreamState {
        self.state
    }

    /// A stream with no device behind it meters nothing, so this is always
    /// [`Levels::SILENT`].
    fn levels(&self) -> Levels {
        Levels::SILENT
    }

    /// A stream with no device behind it has no deadline to miss, so this is
    /// always [`Xruns::NONE`].
    fn xruns(&self) -> Xruns {
        Xruns::NONE
    }

    /// A stream with no device behind it has no deadline to use up, so this is
    /// always [`Headroom::IDLE`].
    fn headroom(&self) -> Headroom {
        Headroom::IDLE
    }

    /// A stream with no device behind it has no callback thread to place, so
    /// this is always [`Placed::UNASKED`].
    fn placement(&self) -> Placed {
        Placed::UNASKED
    }

    /// Nothing, until [`fail`](Self::fail) says otherwise: a device that does
    /// not exist cannot go away on its own.
    fn fault(&self) -> Option<DeviceError> {
        self.faults.read()
    }

    fn start(&mut self) -> Result<(), DeviceError> {
        self.state = StreamState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        self.state = StreamState::Stopped;
        Ok(())
    }
}
