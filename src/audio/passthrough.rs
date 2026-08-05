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
/// carrying `input` of the device's capture channels to `output` of its
/// playback ones.
///
/// A device is opened at whatever width it offers, and rarely at the width a
/// player is using: the selections are which of those channels the path reads
/// and writes. Everything outside them is read past on the way in and silenced
/// on the way out, so a channel nobody chose contributes nothing and hears
/// nothing.
///
/// `slack` is how many frames of silence the playback end starts behind the
/// capture end. The two callbacks are scheduled independently, so without it
/// they race around an empty ring and the playback end is left filling the gaps
/// with silence — audible as crackle rather than as latency. It is paid for in
/// exactly that many frames of delay.
///
/// The ring holds `slack` plus two blocks, which is the slack itself plus room
/// for a block being written while a block is read.
///
/// Everything is allocated here and never again, and each selection is resolved
/// to an offset within a frame once, so this belongs in setup, before the
/// stream starts.
///
/// # Panics
///
/// Panics when `config` states no channels or a block size of zero, when a
/// selection is empty, and when one reaches past the channels `config` says the
/// device has. A path built that way could carry nothing or would read outside
/// the frame it was handed, which is a mistake in setup rather than a condition
/// worth reporting from the real-time thread.
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
/// Taking one channel of the pair takes it at the level it arrived, rather than
/// halving it against a channel nobody is playing:
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
    /// A result below the frame count of `input` means the ring was full and
    /// the rest were dropped: the playback end is not keeping up.
    ///
    /// Only the selected channels are folded; the rest of the frame is read
    /// past. A frame is the mean of those channels rather than their sum,
    /// because a sum clips a source already using the whole range on both. A
    /// source wired to one input of a stereo pair and captured across both
    /// therefore arrives 6 dB down, which is what selecting the one channel
    /// avoids and what input gain is for where there is nothing to select.
    ///
    /// `input` may be longer than the block size the path was built for; it is
    /// taken a block at a time. Samples past the last whole frame are ignored.
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
    /// Fill `output` from the ring, one frame across every channel, and report
    /// how many frames the ring supplied.
    ///
    /// A result below the frame count of `output` means the ring ran dry and
    /// the remaining frames are silence, which is what an underrun sounds like.
    /// Silence rather than whatever the buffer last held: a device hands the
    /// same buffer back repeatedly, so leaving it is a block of audio played
    /// twice.
    ///
    /// The frames land in the head of `output` and are spread across the
    /// selected channels in place, last frame first. A frame sits at or below
    /// the slot it spreads into, so working backwards writes only over slots
    /// already read — which is what lets this end hold no buffer of its own,
    /// and take an `output` of any length. Nothing is left as it was found,
    /// including a channel outside the selection and a trailing part of a
    /// frame: every slot the ring did not fill is silenced, for the same reason
    /// the underrun is.
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
