//! The [`AudioBackend`] implementation backed by real devices.

use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, ErrorKind, SampleFormat, SupportedStreamConfigRange};

use super::{
    AudioBackend, AudioDevice, AudioHost, DeviceError, DuplexStream, FaultReader, Headroom,
    HeadroomReader, LevelReader, Levels, StreamConfig, StreamRequest, StreamState, XrunReader,
    Xruns, fault_channel, headroom_meter, level_meter, passthrough, xrun_counter,
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

fn channel_counts(
    supported: impl Iterator<Item = SupportedStreamConfigRange>,
    sample_rate: u32,
) -> Vec<u16> {
    let mut counts: Vec<u16> = supported
        .filter(|range| {
            range.sample_format() == SampleFormat::F32 && range.contains_rate(sample_rate)
        })
        .map(|range| range.channels())
        .collect();
    counts.sort_unstable();
    counts.dedup();
    counts
}

fn named_device(device: &cpal::Device, channels: Vec<u16>) -> Option<AudioDevice> {
    if channels.is_empty() {
        return None;
    }
    Some(AudioDevice {
        name: device.description().ok()?.name().to_owned(),
        channels,
    })
}

fn input_devices(host: &cpal::Host, sample_rate: u32) -> Vec<AudioDevice> {
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let supported = device.supported_input_configs().ok()?;
            named_device(&device, channel_counts(supported, sample_rate))
        })
        .collect()
}

fn output_devices(host: &cpal::Host, sample_rate: u32) -> Vec<AudioDevice> {
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let supported = device.supported_output_configs().ok()?;
            named_device(&device, channel_counts(supported, sample_rate))
        })
        .collect()
}

impl AudioBackend for CpalBackend {
    type Stream = CpalStream;

    /// Every host `cpal` was built with, not just the default one, because the
    /// default is the guess a settings page exists to replace.
    ///
    /// A device's channel counts are the ones it offers `f32` on at
    /// `sample_rate`, and never the width of a supported range: the ALSA
    /// backend enumerates one range per channel count up to a cap of 64, so an
    /// unfiltered list is an artefact of enumeration rather than a set of
    /// things anyone can open — and on the target board, ALSA is the list a
    /// player would be reading.
    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost> {
        cpal::available_hosts()
            .into_iter()
            .filter_map(|id| {
                let host = cpal::host_from_id(id).ok()?;
                let inputs = input_devices(&host, sample_rate);
                let outputs = output_devices(&host, sample_rate);

                if inputs.is_empty() && outputs.is_empty() {
                    return None;
                }
                Some(AudioHost {
                    name: id.name().to_owned(),
                    inputs,
                    outputs,
                })
            })
            .collect()
    }

    /// The two callbacks are joined by a [`passthrough`] path with one block of
    /// slack, the least that keeps playback from outrunning capture.
    ///
    /// The path is sized from the request, not from what was granted: a stream
    /// wants its callback as it is built and reports its block size only
    /// afterwards, so a device granting a larger block is refused rather than
    /// run against a path too small to feed it.
    ///
    /// Metering happens before the path folds the channels together, so a
    /// channel at full scale cannot hide in the mean of its frame.
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

        let offered_input = channel_counts(
            input.supported_input_configs().map_err(|e| classify(&e))?,
            request.sample_rate,
        );
        let offered_output = channel_counts(
            output
                .supported_output_configs()
                .map_err(|e| classify(&e))?,
            request.sample_rate,
        );

        if !offered_input.contains(&input_channels) || !offered_output.contains(&output_channels) {
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
        let (mut overruns, mut underruns, xruns) = xrun_counter();
        let (mut capture_headroom, capture_load) = headroom_meter(request.sample_rate);
        let (mut render_headroom, render_load) = headroom_meter(request.sample_rate);
        let (reporter, faults) = fault_channel();
        let input_faults = reporter.clone();
        let output_faults = reporter;

        let input_stream = input
            .build_input_stream_raw(
                input_config,
                SampleFormat::F32,
                move |data: &Data, _: &_| {
                    let started = Instant::now();
                    if let Some(samples) = data.as_slice::<f32>() {
                        level_writer.publish(samples);
                        let offered = samples.len() / input_channels as usize;
                        overruns.captured(passthrough_input.capture(samples), offered);
                        capture_headroom.measured(started.elapsed(), offered);
                    }
                },
                move |failed| input_faults.report(classify(&failed)),
                None,
            )
            .map_err(|e| classify(&e))?;

        let output_stream = output
            .build_output_stream_raw(
                output_config,
                SampleFormat::F32,
                move |data: &mut Data, _: &_| {
                    let started = Instant::now();
                    if let Some(samples) = data.as_slice_mut::<f32>() {
                        let wanted = samples.len() / output_channels as usize;
                        underruns.supplied(passthrough_output.render(samples), wanted);
                        render_headroom.measured(started.elapsed(), wanted);
                    }
                },
                move |failed| output_faults.report(classify(&failed)),
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
            xruns,
            capture_load,
            render_load,
            faults,
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
    xruns: XrunReader,
    capture_load: HeadroomReader,
    render_load: HeadroomReader,
    faults: FaultReader,
}

impl DuplexStream for CpalStream {
    /// `block_size` is the output stream's granted size, read back from the
    /// device. `sample_rate` is the requested rate, checked against what the
    /// device offers before opening rather than read back afterwards, because
    /// `cpal` exposes no accessor for it.
    fn config(&self) -> StreamConfig {
        self.config
    }

    /// What [`start`](Self::start) and [`stop`](Self::stop) did, and not a
    /// reading of the device — a stream whose device went away while running
    /// still reports [`StreamState::Running`]. [`fault`](Self::fault) is what
    /// answers whether the device is still there.
    fn state(&self) -> StreamState {
        self.state
    }

    /// Measured on the samples the input device delivered, across every channel
    /// it delivered them on.
    fn levels(&self) -> Levels {
        self.levels.read()
    }

    /// Counted against the passthrough path rather than reported by the device,
    /// so a block the device drops before the callback sees it is invisible
    /// here — the callback is simply not called.
    fn xruns(&self) -> Xruns {
        self.xruns.read()
    }

    /// Timed around everything each callback does with the block it was handed,
    /// which is the work the deadline is against. The wait before the callback
    /// was entered is the host's and is not measured here — a late callback that
    /// then finished quickly reads as headroom, and shows up as an xrun instead.
    fn headroom(&self) -> Headroom {
        self.capture_load.read().worse_of(self.render_load.read())
    }

    /// Whichever of the two streams noticed first. The input and the output are
    /// separate `cpal` streams with an error callback each, and a device that
    /// serves both fails on both — so they report into one latch and the first
    /// report is the one kept.
    fn fault(&self) -> Option<DeviceError> {
        self.faults.read()
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
