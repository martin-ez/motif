//! Carrying a block between the two callbacks, with a ring and a path in
//! between.
//!
//! A duplex device is two streams with two callbacks on two threads, so the
//! route from input to output is not a copy but a hand-off: the capture end
//! writes what the device gave it, the playback end takes whatever is there and
//! asks an [`AudioPath`] what to play. Neither end allocates: both work in
//! buffers built with them, and the path is prepared before either runs.
//!
//! The ring carries one sample per frame, not one per channel. Channel counts
//! differ between the two devices as a matter of course — a mono instrument
//! into a stereo interface is the ordinary case — so the capture end folds a
//! frame down and the playback end spreads it back out, and the path between
//! them is free of the device's channel layout.

use std::ops::Range;

use super::{
    AudioPath, ChannelSelection, SampleConsumer, SampleProducer, StreamConfig, sample_ring,
};

/// Build the two ends of the boundary for a stream at `config`, carrying
/// `input` of its capture channels to `path` and back out over `output`.
///
/// `slack` is how many frames playback starts behind capture, and costs that in
/// delay: without it the two callbacks race around an empty ring and playback
/// crackles. The ring, the scratch and [`AudioPath::prepare`] happen here, once.
///
/// # Panics
///
/// Panics on a block size of zero, or a selection `config` cannot reach.
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
///
/// assert_eq!(played, [1.0, 0.4]);
/// ```
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

    (
        BlockCapture {
            producer,
            channels: config.input_channels as usize,
            selected: selected(input),
            frames: vec![0.0; block].into_boxed_slice(),
        },
        BlockPlayback {
            consumer,
            path,
            channels: config.output_channels as usize,
            selected: selected(output),
            captured: vec![0.0; block].into_boxed_slice(),
            playing: vec![0.0; block].into_boxed_slice(),
        },
    )
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
}

impl BlockCapture {
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

    /// The most frames the boundary can hold between its two ends.
    pub fn capacity(&self) -> usize {
        self.producer.capacity()
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
}

impl<P: AudioPath> BlockPlayback<P> {
    /// Ask the path what to play for the frames the ring supplied, spread the
    /// answer across the selected channels of `output`, and report how many.
    ///
    /// A result below the frame count of `output` means the ring ran dry and the
    /// path was handed silence for the rest. What the path leaves unwritten is
    /// silent too: a device hands the same buffer back, and leaving it plays a
    /// block twice.
    ///
    /// `output` may be any length, worked over in blocks of the size the
    /// boundary was built for. Every slot outside the selection is silenced.
    pub fn render(&mut self, output: &mut [f32]) -> usize {
        let mut supplied = 0;

        for chunk in output.chunks_mut(self.captured.len() * self.channels) {
            let frames = chunk.len() / self.channels;

            let captured = &mut self.captured[..frames];
            let taken = self.consumer.read(captured);
            captured[taken..].fill(0.0);

            let playing = &mut self.playing[..frames];
            playing.fill(0.0);
            self.path.render(captured, playing);

            for (slot, sample) in chunk.chunks_exact_mut(self.channels).zip(playing.iter()) {
                slot.fill(0.0);
                slot[self.selected.clone()].fill(*sample);
            }
            chunk[frames * self.channels..].fill(0.0);

            supplied += taken;
        }

        supplied
    }
}
