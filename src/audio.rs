//! Opening a duplex audio stream and reporting what the device granted.
//!
//! Devices are reached through [`AudioBackend`] rather than directly, so that
//! the rest of the crate compiles against the abstraction and a backend with no
//! hardware behind it can stand in where no audio device exists.

use std::fmt;

mod cpal_backend;

pub use cpal_backend::{CpalBackend, CpalStream};

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
    pub block_size: u32,
    /// Channels the input stream delivers.
    pub input_channels: u16,
    /// Channels the output stream consumes.
    pub output_channels: u16,
}

/// Whether a stream is currently calling back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// The callback is not running.
    Stopped,
    /// The callback is running.
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
