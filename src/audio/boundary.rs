//! Carrying a block between the two callbacks, with a ring and a path in
//! between.
//!
//! A duplex device is two streams with two callbacks on two threads, so the
//! route from input to output is not a copy but a hand-off: the capture end
//! writes what the device gave it, the playback end takes whatever is there and
//! asks an [`AudioPath`] what to play, and [`Priming`] keeps either from being
//! owed anything before the other has run. Neither end allocates: both work in
//! buffers built with them, and the path is prepared before either runs.
//!
//! The ring carries one sample per frame, not one per channel: channel counts
//! differ between the two devices as a matter of course, so the capture end
//! folds a frame down and the playback end spreads it back out, leaving the
//! path between them free of the device's channel layout.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    AudioPath, ChannelSelection, LevelReader, LevelWriter, SampleConsumer, SampleProducer,
    SlackReader, SlackTrim, StreamConfig, Trim, level_meter, sample_ring, slack_hold,
};

const PLAYED_CHANNELS: u16 = 1;

/// Build the two ends of the boundary for a stream at `config`, carrying
/// `input` of its capture channels to `path` and back out over `output`.
///
/// `slack` is how many frames playback starts behind capture, and costs that in
/// delay: without it the two callbacks race around an empty ring and playback
/// crackles. The ring, the scratch and [`AudioPath::prepare`] happen here, once.
///
/// ```
/// use motif::audio::{ChannelSelection, Passthrough, StreamConfig, boundary};
///
/// let (mut input, mut output) = boundary(
///     StreamConfig {
///         sample_rate: 48_000,
///         block_size: 2,
///         input_channels: 2,
///         output_channels: 1,
///     },
///     ChannelSelection::all(2),
///     ChannelSelection::all(1),
///     0,
///     Passthrough::new(),
/// );
/// let mut played = [9.0; 2];
///
/// input.capture(&[1.0, 0.0, 0.4, 0.6]);
/// output.render(&mut played);
///
/// assert_eq!(played, [0.0, 0.0]);
///
/// input.capture(&[1.0, 0.0, 0.4, 0.6]);
/// output.render(&mut played);
///
/// assert_eq!(played, [0.5, 0.5]);
/// ```
///
/// Selecting one channel of the pair takes it at the level it arrived:
///
/// ```
/// use motif::audio::{ChannelSelection, Passthrough, StreamConfig, boundary};
///
/// let (mut input, mut output) = boundary(
///     StreamConfig {
///         sample_rate: 48_000,
///         block_size: 2,
///         input_channels: 2,
///         output_channels: 1,
///     },
///     ChannelSelection { first: 0, count: 1 },
///     ChannelSelection::all(1),
///     0,
///     Passthrough::new(),
/// );
/// let mut played = [0.0; 2];
///
/// input.capture(&[1.0, 0.0, 0.4, 0.6]);
/// output.render(&mut played);
/// input.capture(&[1.0, 0.0, 0.4, 0.6]);
/// output.render(&mut played);
///
/// assert_eq!(played, [1.0, 0.4]);
/// ```
///
/// # Panics
///
/// Panics on a block size of zero, or a selection `config` cannot reach.
pub fn boundary<P: AudioPath>(
    config: StreamConfig,
    input: ChannelSelection,
    output: ChannelSelection,
    slack: usize,
    mut path: P,
) -> (BlockCapture, BlockPlayback<P>) {
    let block = config.block_size as usize;
    assert!(block > 0, "a stream carries nothing block by block");
    assert!(
        config.input_channels > 0 && config.output_channels > 0,
        "a stream carries nothing without channels"
    );
    assert!(
        input.count > 0 && output.count > 0,
        "a stream carries nothing without channels"
    );
    assert!(
        input.reach() <= u32::from(config.input_channels)
            && output.reach() <= u32::from(config.output_channels),
        "a stream cannot reach a channel the device has not got"
    );

    let (mut producer, consumer) = sample_ring(slack + 2 * block);
    producer.write(&vec![0.0; slack]);

    path.prepare(config);
    let priming = Priming::new();
    let (played, metering) = level_meter(PLAYED_CHANNELS, ChannelSelection::all(PLAYED_CHANNELS));
    let (trim, holding) = slack_hold(slack, block);

    (
        BlockCapture {
            producer,
            channels: config.input_channels as usize,
            selected: selected(input),
            frames: vec![0.0; block].into_boxed_slice(),
            priming: priming.clone(),
        },
        BlockPlayback {
            consumer,
            path,
            channels: config.output_channels as usize,
            selected: selected(output),
            captured: vec![0.0; block].into_boxed_slice(),
            playing: vec![0.0; block].into_boxed_slice(),
            slack,
            trim,
            holding,
            priming,
            played,
            metering,
        },
    )
}

/// Whether both ends of a boundary have run since it was last started.
///
/// Two `cpal` streams start independently, so the one calling back first faces
/// a ring the other is not yet servicing and fills or drains it within a couple
/// of blocks. Those frames are lost, but nothing was owed across a half-built
/// boundary — counting them puts an xrun on the board on every healthy start.
///
/// So capture drops what it is handed until playback has run, and playback
/// plays silence until the ring has risen past its slack, each reporting its
/// block whole in the meantime.
///
/// ```
/// use motif::audio::{ChannelSelection, Passthrough, StreamConfig, boundary};
///
/// let (mut input, mut output) = boundary(
///     StreamConfig {
///         sample_rate: 48_000,
///         block_size: 2,
///         input_channels: 1,
///         output_channels: 1,
///     },
///     ChannelSelection::all(1),
///     ChannelSelection::all(1),
///     0,
///     Passthrough::new(),
/// );
/// let mut played = [9.0; 2];
///
/// assert_eq!(input.capture(&[0.1, 0.2]), 2);
/// assert_eq!(output.render(&mut played), 2);
/// assert_eq!(played, [0.0, 0.0]);
///
/// input.capture(&[0.3, 0.4]);
/// output.render(&mut played);
///
/// assert_eq!(played, [0.3, 0.4]);
/// ```
#[derive(Clone)]
pub struct Priming {
    ends: Arc<Ends>,
}

impl Priming {
    /// Put the boundary back to how it starts, so that the next callback on
    /// each end is a start again.
    ///
    /// For the application thread to call while both streams are stopped. A
    /// boundary whose streams are running loses the frames of the callback or
    /// two it takes them to mark themselves again.
    pub fn restart(&self) {
        self.ends.captured.store(false, Ordering::Release);
        self.ends.played.store(false, Ordering::Release);
        self.ends.carrying.store(false, Ordering::Release);
    }

    fn new() -> Self {
        Self {
            ends: Arc::new(Ends {
                captured: AtomicBool::new(false),
                played: AtomicBool::new(false),
                carrying: AtomicBool::new(false),
            }),
        }
    }

    fn capture_ran(&self) {
        self.ends.captured.store(true, Ordering::Release);
    }

    fn playback_ran(&self) {
        self.ends.played.store(true, Ordering::Release);
    }

    fn capture_has_run(&self) -> bool {
        self.ends.captured.load(Ordering::Acquire)
    }

    fn playback_has_run(&self) -> bool {
        self.ends.played.load(Ordering::Acquire)
    }

    fn carrying(&self) -> bool {
        self.ends.carrying.load(Ordering::Acquire)
    }

    fn now_carrying(&self) {
        self.ends.carrying.store(true, Ordering::Release);
    }
}

struct Ends {
    captured: AtomicBool,
    played: AtomicBool,
    carrying: AtomicBool,
}

fn selected(selection: ChannelSelection) -> Range<usize> {
    selection.first as usize..selection.reach() as usize
}

/// The capture end of the boundary, held by the input callback.
pub struct BlockCapture {
    producer: SampleProducer,
    channels: usize,
    selected: Range<usize>,
    frames: Box<[f32]>,
    priming: Priming,
}

impl BlockCapture {
    /// Fold each frame of `input` to a sample on the ring, and report how many.
    ///
    /// A result below the frame count of `input` means the ring was full and the
    /// rest were dropped; samples past its last whole frame are ignored.
    ///
    /// A frame is the mean of the selected channels, not their sum, which would
    /// clip a source using both fully; one wired to a single input arrives 6 dB
    /// down.
    ///
    /// Until the playback end has run, `input` is dropped and reported whole, so
    /// that the ring does not carry the pre-roll for the life of the stream.
    pub fn capture(&mut self, input: &[f32]) -> usize {
        self.priming.capture_ran();
        if !self.priming.playback_has_run() {
            return input.len() / self.channels;
        }

        let mut captured = 0;
        let selected = self.selected.len() as f32;

        for chunk in input.chunks(self.frames.len() * self.channels) {
            let frames = chunk.len() / self.channels;
            let folded = self.frames[..frames].iter_mut();
            for (frame, samples) in folded.zip(chunk.chunks_exact(self.channels)) {
                *frame = samples[self.selected.clone()].iter().sum::<f32>() / selected;
            }
            captured += self.producer.write(&self.frames[..frames]);
        }

        captured
    }

    /// A handle on whether both ends have run, for whatever starts the streams.
    ///
    /// Taken from this end because a callback owns it once the stream is
    /// built, and the two ends share one.
    pub fn priming(&self) -> Priming {
        self.priming.clone()
    }
}

/// The playback end of the boundary, holding the path it plays through.
pub struct BlockPlayback<P> {
    consumer: SampleConsumer,
    path: P,
    channels: usize,
    selected: Range<usize>,
    captured: Box<[f32]>,
    playing: Box<[f32]>,
    slack: usize,
    trim: SlackTrim,
    holding: SlackReader,
    priming: Priming,
    played: LevelWriter,
    metering: LevelReader,
}

impl<P: AudioPath> BlockPlayback<P> {
    /// The magnitude past which a sample is not handed to the device.
    ///
    /// A converter handed an overshoot may clip it or may wrap it, and a wrap
    /// turns a hot mix into a full-scale square wave — the waveform that costs
    /// a tweeter, rather than the overshoot that caused it. Below full scale
    /// the bound is inaudible.
    ///
    /// A sample that is not finite is played as silence: no level is the right
    /// one to make of it, and `f32::clamp` returns NaN for a NaN input where
    /// `min` then `max` would scrub one to full scale.
    pub const FULL_SCALE: f32 = 1.0;

    /// Ask the path what to play for the frames the ring supplied, spread the
    /// answer across the selected channels of `output`, and report how many.
    ///
    /// A result below the frame count of `output` means the ring ran dry and the
    /// path was silent for the rest. What it leaves unwritten is silenced too: a
    /// device hands the same buffer back, and leaving it replays a block.
    ///
    /// `output` may be any length, and slots outside the selection are silenced.
    ///
    /// Until the ring has risen past its slack, `output` is silence reported
    /// whole; so is a block the [`Trim`] padded. See [`Priming`].
    ///
    /// ```
    /// use motif::audio::{ChannelSelection, Passthrough, StreamConfig, boundary};
    ///
    /// let (mut input, mut output) = boundary(
    ///     StreamConfig {
    ///         sample_rate: 48_000,
    ///         block_size: 3,
    ///         input_channels: 1,
    ///         output_channels: 1,
    ///     },
    ///     ChannelSelection::all(1),
    ///     ChannelSelection::all(1),
    ///     0,
    ///     Passthrough::new(),
    /// );
    /// let mut played = [0.0; 3];
    ///
    /// input.capture(&[4.0, -4.0, f32::NAN]);
    /// output.render(&mut played);
    /// input.capture(&[4.0, -4.0, f32::NAN]);
    /// output.render(&mut played);
    ///
    /// assert_eq!(played, [1.0, -1.0, 0.0]);
    /// ```
    pub fn render(&mut self, output: &mut [f32]) -> usize {
        self.priming.playback_ran();
        if !self.priming.carrying() && !self.takes_up_the_slack() {
            output.fill(0.0);
            self.played.silence();
            return output.len() / self.channels;
        }

        let mut holding = self.trimmed(output.len() / self.channels);
        let mut supplied = 0;

        for chunk in output.chunks_mut(self.captured.len() * self.channels) {
            let frames = chunk.len() / self.channels;
            let asked = frames - std::mem::take(&mut holding);

            let captured = &mut self.captured[..frames];
            let taken = self.consumer.read(&mut captured[..asked]);
            captured[taken..].fill(0.0);
            let last = captured[..asked].last().copied().unwrap_or_default();
            captured[asked..].fill(last);

            let playing = &mut self.playing[..frames];
            playing.fill(0.0);
            self.path.render(captured, playing);
            self.played.publish(playing);

            for (slot, sample) in chunk.chunks_exact_mut(self.channels).zip(playing.iter()) {
                slot.fill(0.0);
                slot[self.selected.clone()].fill(Self::bounded(*sample));
            }
            chunk[frames * self.channels..].fill(0.0);

            supplied += if taken < asked { taken } else { frames };
        }

        supplied
    }

    /// A handle on how loud the frames the path played were.
    ///
    /// Taken from this end because a callback owns it once the stream is built,
    /// as [`BlockCapture::priming`] is. Measured on what the path wrote and
    /// before it is spread across the device's channels, so it reads what was
    /// played rather than what a device made of it: a mute upstream reads
    /// silent here while the input goes on arriving.
    pub fn metering(&self) -> LevelReader {
        self.metering.clone()
    }

    /// A handle on the slack the boundary is holding, for whatever reports on
    /// the stream.
    ///
    /// Taken from this end because the trim runs where the ring is read, and
    /// because a callback owns it once the stream is built.
    pub fn slack(&self) -> SlackReader {
        self.holding.clone()
    }

    #[expect(
        clippy::manual_clamp,
        reason = "clamp asserts its bounds, and AGENTS.md invariant 2 keeps a branch that can panic off the audio thread even where the bounds are constant"
    )]
    fn bounded(sample: f32) -> f32 {
        if sample.is_finite() {
            sample.min(Self::FULL_SCALE).max(-Self::FULL_SCALE)
        } else {
            0.0
        }
    }

    fn trimmed(&mut self, wanted: usize) -> usize {
        match self.trim.trim(self.consumer.available(), wanted) {
            Trim::Steady => 0,
            Trim::Drop(frames) => {
                self.consumer.skip(frames);
                0
            }
            Trim::Insert(frames) => frames,
        }
    }

    fn takes_up_the_slack(&mut self) -> bool {
        if !self.priming.capture_has_run() {
            self.consumer
                .skip(self.consumer.available().saturating_sub(self.slack));
            return false;
        }
        if self.consumer.available() <= self.slack {
            return false;
        }

        self.priming.now_carrying();
        true
    }
}
