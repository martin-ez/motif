//! The [`AudioBackend`] implementation backed by real devices.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, ErrorKind, SampleFormat, SupportedStreamConfigRange};

use super::{
    AudioBackend, DeviceError, DuplexStream, LevelReader, Levels, StreamConfig, StreamRequest,
    StreamState, level_meter, passthrough,
};

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

/// Whether the device offers `channels` of `f32` at `sample_rate`.
///
/// The channel count is taken from the device's own default rather than from
/// the widest supported range: the ALSA backend enumerates one range per
/// channel count up to a cap of 64, so the widest range is an artefact of
/// enumeration rather than a configuration anyone wants opened.
fn offers(
    supported: impl Iterator<Item = SupportedStreamConfigRange>,
    sample_rate: u32,
    channels: u16,
) -> bool {
    supported.into_iter().any(|range| {
        range.sample_format() == SampleFormat::F32
            && range.channels() == channels
            && range.contains_rate(sample_rate)
    })
}

impl AudioBackend for CpalBackend {
    type Stream = CpalStream;

    /// The two callbacks are joined by a [`passthrough`] path, so audio at the
    /// input is audible at the output. Its slack is one block, which is the
    /// least give that keeps a playback callback from reading a ring the
    /// capture callback has not reached yet — the two are separate streams, and
    /// nothing orders one against the other.
    ///
    /// The path has to be sized before either stream exists, because a stream
    /// wants its callback at the moment it is built and only reports the block
    /// size it was granted afterwards. It is sized from the request, so a
    /// device that grants a larger block than it was asked for is refused here
    /// rather than run against a path too small to feed it — which would be
    /// audible on every callback for the life of the stream.
    ///
    /// The capture callback also meters what the device handed it, before the
    /// passthrough path folds the channels together. A meter is there to catch
    /// clipping, and a channel at full scale disappears into the mean of a
    /// frame it shares with a quiet one.
    fn open(&self, request: StreamRequest) -> Result<Self::Stream, DeviceError> {
        if request.block_size == 0 {
            return Err(DeviceError::UnsupportedConfig);
        }

        let host = cpal::default_host();
        let input = host
            .default_input_device()
            .ok_or(DeviceError::NoInputDevice)?;
        let output = host
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice)?;

        let input_channels = input
            .default_input_config()
            .map_err(|e| classify(&e))?
            .channels();
        let output_channels = output
            .default_output_config()
            .map_err(|e| classify(&e))?
            .channels();

        if !offers(
            input.supported_input_configs().map_err(|e| classify(&e))?,
            request.sample_rate,
            input_channels,
        ) {
            return Err(DeviceError::UnsupportedConfig);
        }
        if !offers(
            output
                .supported_output_configs()
                .map_err(|e| classify(&e))?,
            request.sample_rate,
            output_channels,
        ) {
            return Err(DeviceError::UnsupportedConfig);
        }

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

        let (mut passthrough_input, mut passthrough_output) = passthrough(
            StreamConfig {
                sample_rate: request.sample_rate,
                block_size: request.block_size,
                input_channels,
                output_channels,
            },
            request.block_size as usize,
        );
        let (mut level_writer, levels) = level_meter();

        let input_stream = input
            .build_input_stream_raw(
                input_config,
                SampleFormat::F32,
                move |data: &Data, _: &_| {
                    if let Some(samples) = data.as_slice::<f32>() {
                        level_writer.publish(samples);
                        passthrough_input.capture(samples);
                    }
                },
                |_| {},
                None,
            )
            .map_err(|e| classify(&e))?;

        let output_stream = output
            .build_output_stream_raw(
                output_config,
                SampleFormat::F32,
                move |data: &mut Data, _: &_| {
                    if let Some(samples) = data.as_slice_mut::<f32>() {
                        passthrough_output.render(samples);
                    }
                },
                |_| {},
                None,
            )
            .map_err(|e| classify(&e))?;

        let block_size = output_stream.buffer_size().map_err(|e| classify(&e))?;
        let captured_block_size = input_stream.buffer_size().map_err(|e| classify(&e))?;

        if block_size > request.block_size || captured_block_size > request.block_size {
            return Err(DeviceError::UnsupportedConfig);
        }

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
            levels,
        })
    }
}

/// A pair of `cpal` streams started and stopped together.
pub struct CpalStream {
    config: StreamConfig,
    state: StreamState,
    input: cpal::Stream,
    output: cpal::Stream,
    levels: LevelReader,
}

impl DuplexStream for CpalStream {
    /// `block_size` is the output stream's granted size, read back from the
    /// device. `sample_rate` is the requested rate, checked against what the
    /// device offers before opening rather than read back afterwards, because
    /// `cpal` exposes no accessor for it.
    fn config(&self) -> StreamConfig {
        self.config
    }

    /// What [`start`](Self::start) and [`stop`](Self::stop) did. It is not a
    /// reading of the device: a device that goes away while running is reported
    /// through the stream's error callback, which this type does not observe.
    fn state(&self) -> StreamState {
        self.state
    }

    /// Measured on the samples the input device delivered, across every channel
    /// it delivered them on.
    fn levels(&self) -> Levels {
        self.levels.read()
    }

    /// Both streams are acted on before either error is returned, so one of
    /// them failing never leaves the other untouched. The state is the
    /// conservative reading of what happened: [`StreamState::Running`] if
    /// either stream may be calling back.
    fn start(&mut self) -> Result<(), DeviceError> {
        let input = self.input.play();
        let output = self.output.play();

        if input.is_ok() || output.is_ok() {
            self.state = StreamState::Running;
        }

        input.map_err(|e| classify(&e))?;
        output.map_err(|e| classify(&e))?;
        Ok(())
    }

    /// Stopping reports [`StreamState::Stopped`] only once both streams have
    /// confirmed they paused, so a partial failure leaves the state saying a
    /// callback may still be running, which is the assumption that is safe to
    /// hold.
    fn stop(&mut self) -> Result<(), DeviceError> {
        let input = self.input.pause();
        let output = self.output.pause();

        if input.is_ok() && output.is_ok() {
            self.state = StreamState::Stopped;
        }

        input.map_err(|e| classify(&e))?;
        output.map_err(|e| classify(&e))?;
        Ok(())
    }
}
