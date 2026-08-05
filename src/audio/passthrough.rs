//! Copying captured audio to the output, with a ring in between.
//!
//! A duplex device is two streams with two callbacks on two threads, so the
//! path from input to output is not a copy but a hand-off: the capture end
//! writes what the device gave it, the playback end takes whatever is there.
//! Neither end allocates: the capture end folds into a buffer built with the
//! path, and the playback end works in the buffer the device handed it.
//!
//! The ring carries one sample per frame, not one per channel. Channel counts
//! differ between the two devices as a matter of course — a mono instrument
//! into a stereo interface is the ordinary case — so the capture end folds a
//! frame down and the playback end spreads it back out, and everything between
//! them is free of the device's channel layout.

use std::ops::Range;

use super::{ChannelSelection, SampleConsumer, SampleProducer, StreamConfig, sample_ring};

/// Build the two ends of a passthrough path for a stream running at `config`,
/// carrying `input` of its capture channels to `output` of its playback ones.
///
/// `slack` is how many frames playback starts behind capture, and costs that in
/// delay: without it the two independent callbacks race around an empty ring and
/// playback crackles. The ring holds `slack` plus two blocks, allocated once.
///
/// # Panics
///
/// Panics on a block size of zero, or a selection `config` cannot reach.
///
/// ```
/// use motif::audio::{ChannelSelection, StreamConfig, passthrough};
///
/// let (mut input, mut output) = passthrough(
///     StreamConfig {
///         sample_rate: 48_000,
///         block_size: 2,
///         input_channels: 2,
///         output_channels: 1,
///     },
///     ChannelSelection::all(2),
///     ChannelSelection::all(1),
///     0,
/// );
/// let mut played = [0.0; 2];
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
/// use motif::audio::{ChannelSelection, StreamConfig, passthrough};
///
/// let (mut input, mut output) = passthrough(
///     StreamConfig {
///         sample_rate: 48_000,
///         block_size: 2,
///         input_channels: 2,
///         output_channels: 1,
///     },
///     ChannelSelection { first: 0, count: 1 },
///     ChannelSelection::all(1),
///     0,
/// );
/// let mut played = [0.0; 2];
///
/// input.capture(&[1.0, 0.0, 0.4, 0.6]);
/// output.render(&mut played);
///
/// assert_eq!(played, [1.0, 0.4]);
/// ```
pub fn passthrough(
    config: StreamConfig,
    input: ChannelSelection,
    output: ChannelSelection,
    slack: usize,
) -> (PassthroughInput, PassthroughOutput) {
    let block = config.block_size as usize;
    assert!(
        block > 0,
        "a passthrough path carries nothing block by block"
    );
    assert!(
        config.input_channels > 0 && config.output_channels > 0,
        "a passthrough path carries nothing without channels"
    );
    assert!(
        input.count > 0 && output.count > 0,
        "a passthrough path carries nothing without channels"
    );
    assert!(
        input.reach() <= u32::from(config.input_channels)
            && output.reach() <= u32::from(config.output_channels),
        "a passthrough path cannot reach a channel the device has not got"
    );

    let (mut producer, consumer) = sample_ring(slack + 2 * block);
    producer.write(&vec![0.0; slack]);

    (
        PassthroughInput {
            producer,
            channels: config.input_channels as usize,
            selected: selected(input),
            frames: vec![0.0; block].into_boxed_slice(),
        },
        PassthroughOutput {
            consumer,
            channels: config.output_channels as usize,
            selected: selected(output),
        },
    )
}

fn selected(selection: ChannelSelection) -> Range<usize> {
    selection.first as usize..selection.reach() as usize
}

/// The capture end of a passthrough path, held by the input callback.
pub struct PassthroughInput {
    producer: SampleProducer,
    channels: usize,
    selected: Range<usize>,
    frames: Box<[f32]>,
}

impl PassthroughInput {
    /// Fold each frame of `input` to one sample and write it to the ring, and
    /// report how many frames that was.
    ///
    /// A result below the frame count of `input` means the ring was full and the
    /// rest were dropped. `input` may be longer than the block size it was built
    /// for; samples past the last whole frame are ignored.
    ///
    /// A frame is the mean of the selected channels, not their sum, which would
    /// clip a source using the whole range on both — and one wired to a single
    /// input of a pair, captured across both, arrives 6 dB down.
    pub fn capture(&mut self, input: &[f32]) -> usize {
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

    /// The most frames the path can hold between its two ends.
    pub fn capacity(&self) -> usize {
        self.producer.capacity()
    }
}

/// The playback end of a passthrough path, held by the output callback.
pub struct PassthroughOutput {
    consumer: SampleConsumer,
    channels: usize,
    selected: Range<usize>,
}

impl PassthroughOutput {
    /// Fill `output` from the ring, one frame across the selected channels, and
    /// report how many frames the ring supplied.
    ///
    /// A result below the frame count of `output` means the ring ran dry and the
    /// rest is silence — silence rather than what the buffer last held, since a
    /// device hands the same buffer back and leaving it plays a block twice.
    ///
    /// Frames land in the head of `output` and are spread in place, last frame
    /// first, so this end holds no buffer of its own and takes an `output` of
    /// any length. Every slot outside the selection is silenced too.
    pub fn render(&mut self, output: &mut [f32]) -> usize {
        let frames = output.len() / self.channels;
        let supplied = self.consumer.read(&mut output[..frames]);
        output[supplied..frames].fill(0.0);

        for frame in (0..frames).rev() {
            let sample = output[frame];
            let slot = &mut output[frame * self.channels..][..self.channels];
            slot.fill(0.0);
            slot[self.selected.clone()].fill(sample);
        }
        output[frames * self.channels..].fill(0.0);

        supplied
    }
}
