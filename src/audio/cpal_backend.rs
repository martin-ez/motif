//! The [`AudioBackend`] implementation backed by real devices.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, ErrorKind, SampleFormat, SupportedStreamConfigRange};

use super::{AudioBackend, DeviceError, DuplexStream, StreamConfig, StreamRequest, StreamState};

/// Audio devices reached through `cpal`.
///
/// The input and output streams are separate, because `cpal` has no duplex API:
/// they are opened from the same request and started together, but the host
/// drives each with its own callback.
pub struct CpalBackend;

impl CpalBackend {
    /// A backend over the host's default audio API.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn classify(error: &cpal::Error) -> DeviceError {
    match error.kind() {
        ErrorKind::DeviceNotAvailable | ErrorKind::DeviceBusy => DeviceError::DeviceNotAvailable,
        ErrorKind::PermissionDenied => DeviceError::PermissionDenied,
        ErrorKind::UnsupportedConfig
        | ErrorKind::UnsupportedOperation
        | ErrorKind::InvalidInput => DeviceError::UnsupportedConfig,
        _ => DeviceError::BackendFailure,
    }
}

fn channels_at(
    supported: impl Iterator<Item = SupportedStreamConfigRange>,
    sample_rate: u32,
) -> Option<u16> {
    supported
        .filter(|range| range.sample_format() == SampleFormat::F32)
        .filter(|range| range.contains_rate(sample_rate))
        .map(|range| range.channels())
        .max()
}

impl AudioBackend for CpalBackend {
    type Stream = CpalStream;

    fn open(&self, request: StreamRequest) -> Result<Self::Stream, DeviceError> {
        let host = cpal::default_host();
        let input = host
            .default_input_device()
            .ok_or(DeviceError::NoInputDevice)?;
        let output = host
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice)?;

        let input_channels = channels_at(
            input.supported_input_configs().map_err(|e| classify(&e))?,
            request.sample_rate,
        )
        .ok_or(DeviceError::UnsupportedConfig)?;
        let output_channels = channels_at(
            output
                .supported_output_configs()
                .map_err(|e| classify(&e))?,
            request.sample_rate,
        )
        .ok_or(DeviceError::UnsupportedConfig)?;

        let input_config = cpal::StreamConfig {
            channels: input_channels,
            sample_rate: request.sample_rate,
            buffer_size: cpal::BufferSize::Fixed(request.block_size),
        };
        let output_config = cpal::StreamConfig {
            channels: output_channels,
            sample_rate: request.sample_rate,
            buffer_size: cpal::BufferSize::Fixed(request.block_size),
        };

        let input_stream = input
            .build_input_stream_raw(
                input_config,
                SampleFormat::F32,
                |_: &Data, _: &_| {},
                |_| {},
                None,
            )
            .map_err(|e| classify(&e))?;

        let output_stream = output
            .build_output_stream_raw(
                output_config,
                SampleFormat::F32,
                |data: &mut Data, _: &_| {
                    if let Some(samples) = data.as_slice_mut::<f32>() {
                        samples.fill(0.0);
                    }
                },
                |_| {},
                None,
            )
            .map_err(|e| classify(&e))?;

        let block_size = output_stream.buffer_size().map_err(|e| classify(&e))?;

        Ok(CpalStream {
            config: StreamConfig {
                sample_rate: request.sample_rate,
                block_size,
                input_channels,
                output_channels,
            },
            state: StreamState::Stopped,
            input: input_stream,
            output: output_stream,
        })
    }
}

/// A pair of `cpal` streams started and stopped together.
pub struct CpalStream {
    config: StreamConfig,
    state: StreamState,
    input: cpal::Stream,
    output: cpal::Stream,
}

impl DuplexStream for CpalStream {
    /// The granted configuration. `block_size` is the output stream's, which is
    /// the one an underrun is audible on.
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn state(&self) -> StreamState {
        self.state
    }

    fn start(&mut self) -> Result<(), DeviceError> {
        self.input.play().map_err(|e| classify(&e))?;
        self.output.play().map_err(|e| classify(&e))?;
        self.state = StreamState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        self.input.pause().map_err(|e| classify(&e))?;
        self.output.pause().map_err(|e| classify(&e))?;
        self.state = StreamState::Stopped;
        Ok(())
    }
}
