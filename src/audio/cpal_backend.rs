//! The [`AudioBackend`] implementation backed by real devices.
//!
//! `cpal` hands out no identifier of its own, so a [`DeviceId`] is a name and
//! the place of the device among those sharing it, counted in the host's own
//! enumeration order. Listing and opening walk that order and count the same
//! way, before any filter by rate or format, so an identifier a listing gave
//! out reaches the device that listing meant.

use std::collections::HashMap;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, ErrorKind, SampleFormat, SupportedStreamConfigRange};

use super::{
    AudioBackend, AudioDevice, AudioHost, AudioPath, ChannelSelection, DeviceError, DeviceId,
    DeviceSelection, DuplexStream, FaultReader, FaultReporter, Grant, Headroom, HeadroomReader,
    LevelReader, Levels, Placed, Placement, Priming, PriorityReader, PriorityReporter,
    StreamConfig, StreamRequest, StreamState, XrunReader, Xruns, boundary, fault_channel,
    headroom_meter, level_meter, opened_width, pinning, priority_latch, xrun_counter,
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

fn reported(error: &cpal::Error, faults: &FaultReporter, denials: &PriorityReporter) {
    match error.kind() {
        ErrorKind::RealtimeDenied => denials.denied(),
        _ => faults.report(classify(error)),
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

fn offering_device(id: DeviceId, channels: Vec<u16>) -> Option<AudioDevice> {
    if channels.is_empty() {
        return None;
    }
    Some(AudioDevice { id, channels })
}

fn offered_selection(offered: &[u16], preferred: u16) -> Option<ChannelSelection> {
    let widest = *offered.last()?;
    Some(ChannelSelection::all(preferred.min(widest).max(1)))
}

fn listed_default(
    listed: Vec<AudioDevice>,
    default: Option<&cpal::Device>,
    preferred: Option<u16>,
) -> Option<(DeviceId, ChannelSelection)> {
    let named = default
        .and_then(|device| device.description().ok())
        .map(|description| description.name().to_owned());

    let device = named
        .and_then(|name| listed.iter().find(|device| device.id.name == name))
        .or_else(|| listed.first())?;

    let width = preferred.unwrap_or(*device.channels.last()?);
    Some((
        device.id.clone(),
        offered_selection(&device.channels, width)?,
    ))
}

fn default_width(config: Option<cpal::SupportedStreamConfig>) -> Option<u16> {
    config.map(|config| config.channels())
}

fn host_named(name: &str) -> Option<cpal::Host> {
    cpal::available_hosts()
        .into_iter()
        .find(|id| id.name() == name)
        .and_then(|id| cpal::host_from_id(id).ok())
}

fn identified(
    devices: impl Iterator<Item = cpal::Device>,
) -> impl Iterator<Item = (DeviceId, cpal::Device)> {
    let mut counted: HashMap<String, usize> = HashMap::new();

    devices.filter_map(move |device| {
        let name = device.description().ok()?.name().to_owned();
        let nth = counted.entry(name.clone()).or_default();
        let id = DeviceId { name, nth: *nth };
        *nth += 1;
        Some((id, device))
    })
}

fn device_identified(
    devices: impl Iterator<Item = cpal::Device>,
    id: &DeviceId,
) -> Option<cpal::Device> {
    identified(devices).find_map(|(found, device)| (found == *id).then_some(device))
}

fn input_devices(host: &cpal::Host, sample_rate: u32) -> Vec<AudioDevice> {
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    identified(devices)
        .filter_map(|(id, device)| {
            let supported = device.supported_input_configs().ok()?;
            offering_device(id, channel_counts(supported, sample_rate))
        })
        .collect()
}

fn output_devices(host: &cpal::Host, sample_rate: u32) -> Vec<AudioDevice> {
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    identified(devices)
        .filter_map(|(id, device)| {
            let supported = device.supported_output_configs().ok()?;
            offering_device(id, channel_counts(supported, sample_rate))
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

    /// The default host's default devices across the width their own default
    /// configuration uses, falling back to the first device listed.
    ///
    /// Named out of [`hosts`](Self::hosts) rather than off the default device,
    /// which are not always the same name for one thing: ALSA's default device
    /// describes itself as `Default Audio Device` while enumerating that PCM
    /// gives whatever its hint says, so a name taken from the device is one
    /// [`open`](Self::open) cannot find.
    fn defaults(&self, sample_rate: u32) -> Option<DeviceSelection> {
        let host = cpal::default_host();
        let default_input = host.default_input_device();
        let default_output = host.default_output_device();

        let (input, input_channels) = listed_default(
            input_devices(&host, sample_rate),
            default_input.as_ref(),
            default_width(
                default_input
                    .as_ref()
                    .and_then(|device| device.default_input_config().ok()),
            ),
        )?;
        let (output, output_channels) = listed_default(
            output_devices(&host, sample_rate),
            default_output.as_ref(),
            default_width(
                default_output
                    .as_ref()
                    .and_then(|device| device.default_output_config().ok()),
            ),
        )?;

        Some(DeviceSelection {
            host: host.id().name().to_owned(),
            input,
            input_channels,
            output,
            output_channels,
        })
    }

    /// The two callbacks are joined by a [`boundary`] running `path`, with one
    /// block of slack — the least that keeps playback from outrunning capture.
    ///
    /// The boundary and the path are sized from the request, not from what was
    /// granted: a stream wants its callback as it is built and reports its block
    /// size afterwards, so a device granting a larger block is refused instead.
    ///
    /// Metering covers the selected channels and happens before they are folded,
    /// so one at full scale hides neither in the mean of its frame nor behind a
    /// hot line on an input nobody selected.
    fn open<P: AudioPath>(
        &self,
        selection: &DeviceSelection,
        request: StreamRequest,
        path: P,
    ) -> Result<Self::Stream, DeviceError> {
        if request.block_size == 0 {
            return Err(DeviceError::UnsupportedConfig);
        }

        let host = host_named(&selection.host).ok_or(DeviceError::NoSuchHost)?;
        let input = device_identified(
            host.input_devices().map_err(|e| classify(&e))?,
            &selection.input,
        )
        .ok_or(DeviceError::NoInputDevice)?;
        let output = device_identified(
            host.output_devices().map_err(|e| classify(&e))?,
            &selection.output,
        )
        .ok_or(DeviceError::NoOutputDevice)?;

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

        let natural_input = default_width(input.default_input_config().ok()).unwrap_or_default();
        let natural_output = default_width(output.default_output_config().ok()).unwrap_or_default();

        let input_channels = opened_width(&offered_input, selection.input_channels, natural_input)
            .ok_or(DeviceError::UnsupportedConfig)?;
        let output_channels =
            opened_width(&offered_output, selection.output_channels, natural_output)
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

        let (mut capture, mut playback) = boundary(
            StreamConfig {
                sample_rate: request.sample_rate,
                block_size: request.block_size,
                input_channels,
                output_channels,
            },
            selection.input_channels,
            selection.output_channels,
            request.block_size as usize,
            path,
        );
        let priming = capture.priming();
        let played = playback.metering();
        let (mut level_writer, levels) = level_meter(input_channels, selection.input_channels);
        let (mut overruns, mut underruns, xruns) = xrun_counter();
        let (mut capture_headroom, capture_load) = headroom_meter(request.sample_rate);
        let (mut render_headroom, render_load) = headroom_meter(request.sample_rate);
        let (reporter, faults) = fault_channel();
        let input_faults = reporter.clone();
        let output_faults = reporter;
        let (denials, priority) = priority_latch();
        let input_denials = denials.clone();
        let output_denials = denials;

        let (affinity, built) = pinning(Placement::available(), || {
            let input_stream = input.build_input_stream_raw(
                input_config,
                SampleFormat::F32,
                move |data: &Data, _: &_| {
                    let started = Instant::now();
                    if let Some(samples) = data.as_slice::<f32>() {
                        level_writer.publish(samples);
                        let offered = samples.len() / input_channels as usize;
                        overruns.captured(capture.capture(samples), offered);
                        capture_headroom.measured(started.elapsed(), offered);
                    }
                },
                move |failed| reported(&failed, &input_faults, &input_denials),
                None,
            );
            let output_stream = output.build_output_stream_raw(
                output_config,
                SampleFormat::F32,
                move |data: &mut Data, _: &_| {
                    let started = Instant::now();
                    if let Some(samples) = data.as_slice_mut::<f32>() {
                        let wanted = samples.len() / output_channels as usize;
                        underruns.supplied(playback.render(samples), wanted);
                        render_headroom.measured(started.elapsed(), wanted);
                    }
                },
                move |failed| reported(&failed, &output_faults, &output_denials),
                None,
            );

            (input_stream, output_stream)
        });

        let (input_stream, output_stream) = built;
        let input_stream = input_stream.map_err(|e| classify(&e))?;
        let output_stream = output_stream.map_err(|e| classify(&e))?;

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
            played,
            xruns,
            capture_load,
            render_load,
            affinity,
            priority,
            faults,
            priming,
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
    played: LevelReader,
    xruns: XrunReader,
    capture_load: HeadroomReader,
    render_load: HeadroomReader,
    affinity: Grant,
    priority: PriorityReader,
    faults: FaultReader,
    priming: Priming,
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

    /// Measured on the samples the input device delivered, across the channels
    /// the selection names and no others — the ones the path goes on to
    /// capture.
    fn captured(&self) -> Levels {
        self.levels.read()
    }

    /// Measured at the boundary, on the frames the path wrote and before they
    /// are spread across the output channels: the level a player set rather
    /// than the one the converter saw.
    fn played(&self) -> Levels {
        self.played.read()
    }

    /// Counted against the boundary rather than reported by the device,
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

    /// The core is asked for while the streams are built and inherited by both
    /// callback threads, so this answers for the pair and no syscall is made on
    /// either. The scheduling class is whatever the layer below granted.
    fn placement(&self) -> Placed {
        Placed {
            affinity: self.affinity,
            priority: self.priority.read(),
        }
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
    ///
    /// The boundary is primed first, because the two streams begin calling back
    /// independently and a device can take tens of milliseconds over it — long
    /// enough for the other end to fill or drain the ring several times over.
    fn start(&mut self) -> Result<(), DeviceError> {
        self.priming.restart();

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
