//! Opening a duplex audio stream, and the boundary between its callback and
//! the rest of the system.
//!
//! Devices are reached through [`AudioBackend`] rather than directly, so that
//! the rest of the crate compiles against the abstraction and a backend with no
//! hardware behind it can stand in where no audio device exists. [`sample_ring`]
//! carries samples between two threads without locking or allocating, which is
//! what a real-time callback needs of anything it touches; [`passthrough`] is
//! what a duplex stream's two callbacks do with one, and [`command_channel`]
//! carries changes the other way.

use std::fmt;

mod command;
mod cpal_backend;
mod passthrough;
mod ring;

pub use command::{Command, CommandReceiver, CommandSender, SendError, command_channel};
pub use cpal_backend::{CpalBackend, CpalStream};
pub use passthrough::{PassthroughInput, PassthroughOutput, passthrough};
pub use ring::{SampleConsumer, SampleProducer, sample_ring};

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

/// A source of duplex audio streams.
pub trait AudioBackend {
    /// The stream this backend opens.
    type Stream: DuplexStream;

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

impl AudioBackend for NullBackend {
    type Stream = NullStream;

    fn open(&self, request: StreamRequest) -> Result<Self::Stream, DeviceError> {
        let matches_exactly = request.sample_rate == self.granted.sample_rate
            && request.block_size == self.granted.block_size;
        if self.rejects_mismatch && !matches_exactly {
            return Err(DeviceError::UnsupportedConfig);
        }

        Ok(NullStream {
            config: self.granted,
            state: StreamState::Stopped,
        })
    }
}

/// A stream that moves no samples, opened by [`NullBackend`].
pub struct NullStream {
    config: StreamConfig,
    state: StreamState,
}

impl DuplexStream for NullStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn state(&self) -> StreamState {
        self.state
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
