//! Opening a duplex audio stream, and the boundary between its callback and
//! the rest of the system.
//!
//! Devices are reached through [`AudioBackend`] rather than directly, so that
//! the rest of the crate compiles against the abstraction and a backend with no
//! hardware behind it can stand in where no audio device exists. [`sample_ring`]
//! carries samples between two threads without locking or allocating, which is
//! what a real-time callback needs of anything it touches; [`passthrough`] is
//! what a duplex stream's two callbacks do with one, [`command_channel`] carries
//! changes the other way, and [`level_meter`] sends back the other thing that
//! crosses the boundary: not the audio itself but how loud it was.
//!
//! [`xrun_counter`] carries the news that the boundary failed, which none of
//! the others can report: the samples a dropout costs are gone.
//!
//! [`fault_channel`] carries the news that the device did. That one outlives
//! the stream it came from, so [`DeviceLink`] holds the pieces needed to open
//! another and is what the rest of the application talks to.

use std::fmt;

mod command;
mod cpal_backend;
mod fault;
mod level;
mod link;
mod passthrough;
mod ring;
mod xrun;

pub use command::{Command, CommandReceiver, CommandSender, SendError, command_channel};
pub use cpal_backend::{CpalBackend, CpalStream};
pub use fault::{FaultReader, FaultReporter, fault_channel};
pub use level::{LevelReader, LevelWriter, Levels, level_meter};
pub use link::{AudioState, DeviceLink};
pub use passthrough::{PassthroughInput, PassthroughOutput, passthrough};
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

/// A device, and the channel counts it can be opened with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    /// What the host calls this device.
    pub name: String,
    /// Channel counts the device can be opened with, ascending and without
    /// repeats.
    pub channels: Vec<u16>,
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
    /// The host offers no input device.
    NoInputDevice,
    /// The host offers no output device.
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
            Self::NoInputDevice => "the host offers no input device",
            Self::NoOutputDevice => "the host offers no output device",
            Self::UnsupportedConfig => "the device cannot run at the requested configuration",
            Self::DeviceNotAvailable => "the device is not available",
            Self::PermissionDenied => "access to the device was refused",
            Self::BackendFailure => "the audio backend failed",
        };
        f.write_str(described)
    }
}

impl std::error::Error for DeviceError {}

/// A source of duplex audio streams, and a description of what there is to
/// open.
pub trait AudioBackend {
    /// The stream this backend opens.
    type Stream: DuplexStream;

    /// The hosts and devices there are to open at `sample_rate`.
    ///
    /// Listed means openable: a device that comes back offers `f32` at
    /// `sample_rate` on every channel count it lists, and one that cannot meet
    /// that is absent rather than listed and unopenable. A host left with no
    /// devices is absent too, being a row with nothing behind it. Channel
    /// counts ascend without repeats.
    ///
    /// Only that direction is promised. A backend may still open something it
    /// did not list — [`NullBackend::rounding`] grants its own configuration
    /// whatever it is asked for — so this is a menu to choose from rather than
    /// a ruling on what would work.
    ///
    /// A host that will not open, or a device that will not answer, drops out
    /// of the list rather than failing the call. There is nothing a caller can
    /// do with that distinction it cannot do with a shorter list.
    ///
    /// This talks to the host, so it blocks and allocates, and must never be
    /// reached from the audio callback.
    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost>;

    /// Open an input and an output stream at `request`.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError`] when no device is available or the device cannot
    /// run at the requested configuration. A device that cannot meet the
    /// request never panics.
    fn open(&self, request: StreamRequest) -> Result<Self::Stream, DeviceError>;
}

/// An input and an output stream running together on one device.
pub trait DuplexStream {
    /// The configuration the device granted, which may differ from the request.
    fn config(&self) -> StreamConfig;

    /// Whether the callback is running.
    fn state(&self) -> StreamState;

    /// How loud the most recent block of input was.
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
}

impl NullBackend {
    /// A backend whose device grants its own configuration whatever is asked
    /// of it, modelling a device that rounds a request to what it supports.
    pub fn rounding(granted: StreamConfig) -> Self {
        Self {
            granted,
            rejects_mismatch: false,
        }
    }

    /// A backend whose device refuses any request it cannot meet exactly,
    /// modelling a device that supports one configuration and no other.
    pub fn rejecting(granted: StreamConfig) -> Self {
        Self {
            granted,
            rejects_mismatch: true,
        }
    }
}

const NULL_HOST: &str = "null";
const NULL_INPUT: &str = "null input";
const NULL_OUTPUT: &str = "null output";

fn null_device(name: &str, channels: u16) -> Vec<AudioDevice> {
    if channels == 0 {
        return Vec::new();
    }
    vec![AudioDevice {
        name: name.to_owned(),
        channels: vec![channels],
    }]
}

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
            inputs: null_device(NULL_INPUT, self.granted.input_channels),
            outputs: null_device(NULL_OUTPUT, self.granted.output_channels),
        };

        if host.inputs.is_empty() && host.outputs.is_empty() {
            return Vec::new();
        }
        vec![host]
    }

    fn open(&self, request: StreamRequest) -> Result<Self::Stream, DeviceError> {
        let matches_exactly = request.sample_rate == self.granted.sample_rate
            && request.block_size == self.granted.block_size;
        if self.rejects_mismatch && !matches_exactly {
            return Err(DeviceError::UnsupportedConfig);
        }

        let (reporter, faults) = fault_channel();

        Ok(NullStream {
            config: self.granted,
            state: StreamState::Stopped,
            reporter,
            faults,
        })
    }
}

/// A stream that moves no samples, opened by [`NullBackend`].
pub struct NullStream {
    config: StreamConfig,
    state: StreamState,
    reporter: FaultReporter,
    faults: FaultReader,
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
}

impl DuplexStream for NullStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn state(&self) -> StreamState {
        self.state
    }

    /// A stream that moves no samples has nothing to measure, so this is always
    /// [`Levels::SILENT`].
    fn levels(&self) -> Levels {
        Levels::SILENT
    }

    /// A stream that moves no samples can lose none, so this is always
    /// [`Xruns::NONE`].
    fn xruns(&self) -> Xruns {
        Xruns::NONE
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
