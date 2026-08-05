//! How many frames the device has moved, published from the callback to the
//! threads that time things against it.
//!
//! Captured audio is timestamped in frames, so anything meant to line up with
//! it has to be timestamped the same way — a wall clock read on the application
//! thread drifts against the device and cannot be compared with a sample
//! position at all. The callback is the only thread that knows how many frames
//! have gone by, so it is the one that publishes them.
//!
//! Publishing is one relaxed load and one release store of a counter that is
//! already there, which is all the callback may spend on a clock nobody else's
//! deadline depends on (invariant 2). [`Counting`] is what does the publishing:
//! the counting end belongs to whatever plays, since a block is the only thing
//! that knows how many frames went by.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{AudioPath, StreamConfig};

/// Build a sample clock counting at `sample_rate`, and split it into the end
/// that counts and the end that reads.
///
/// The rate is the stream's own, from
/// [`StreamConfig`](crate::audio::StreamConfig) rather than from what was asked
/// for, and it travels with the clock so that a reader turning frames into a
/// duration cannot pair them with a rate the device never granted.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// use motif::audio::sample_clock;
///
/// let (mut elapsed, now) = sample_clock(48_000);
///
/// elapsed.advance(128);
/// elapsed.advance(128);
///
/// assert_eq!(now.read(), 256);
/// assert_eq!(now.sample_rate(), 48_000);
/// ```
pub fn sample_clock(sample_rate: u32) -> (SampleClockWriter, SampleClockReader) {
    let counted = Arc::new(AtomicU64::new(0));

    (
        SampleClockWriter {
            counted: Arc::clone(&counted),
        },
        SampleClockReader {
            counted,
            sample_rate,
        },
    )
}

/// The counting end of a sample clock, held by the audio callback.
///
/// `&mut self` on the one method that writes is what makes this the only
/// counting end: two callbacks each adding their own blocks would count the
/// same frames twice.
pub struct SampleClockWriter {
    counted: Arc<AtomicU64>,
}

impl SampleClockWriter {
    /// Count `frames` more, and report the count they reached.
    ///
    /// Call it once per block, with the frames that block covered. The count
    /// saturates rather than wrapping, which at any rate a device runs at is
    /// several million years away and still better than a clock that goes
    /// backwards.
    pub fn advance(&mut self, frames: usize) -> u64 {
        let reached = self
            .counted
            .load(Ordering::Relaxed)
            .saturating_add(frames as u64);
        self.counted.store(reached, Ordering::Release);

        reached
    }
}

/// A path with a sample clock on it, counting the frames it plays.
///
/// The writer goes to whatever renders on the callback, wrapped around it
/// rather than held by it, so a loop engine, a monitor and a passthrough all
/// keep the clock without knowing there is one.
///
/// It counts the frames both slices carried: two lengths are a mismatch nothing
/// on the audio thread may panic over, and counting frames that were never
/// played would put a tap ahead of the audio it was tapped against.
///
/// ```
/// use motif::audio::{AudioPath, Counting, Passthrough, sample_clock};
///
/// let (frames, elapsed) = sample_clock(48_000);
/// let mut path = Counting::new(frames, Passthrough::new());
///
/// path.render(&[0.25; 128], &mut [0.0; 128]);
///
/// assert_eq!(elapsed.read(), 128);
/// ```
pub struct Counting<P> {
    elapsed: SampleClockWriter,
    path: P,
}

impl<P: AudioPath> Counting<P> {
    /// A path playing what `path` plays, counting the frames onto `elapsed`.
    pub const fn new(elapsed: SampleClockWriter, path: P) -> Self {
        Self { elapsed, path }
    }
}

impl<P: AudioPath> AudioPath for Counting<P> {
    /// Nothing of its own to prepare: the clock counts frames whatever rate
    /// they arrive at, and the configuration goes on to the path it wraps.
    fn prepare(&mut self, config: StreamConfig) {
        self.path.prepare(config);
    }

    /// Plays the block, then counts it — the count reads as frames the device
    /// has finished with rather than frames it is partway through.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.path.render(captured, playing);
        self.elapsed.advance(captured.len().min(playing.len()));
    }
}

/// The reading end of a sample clock, held by whichever thread timestamps
/// against it.
pub struct SampleClockReader {
    counted: Arc<AtomicU64>,
    sample_rate: u32,
}

impl SampleClockReader {
    /// How many frames the device has moved since the clock was made.
    ///
    /// Reading takes nothing: the count only ever grows, so two reads that find
    /// the same value mean no block landed between them rather than that one
    /// was consumed.
    pub fn read(&self) -> u64 {
        self.counted.load(Ordering::Acquire)
    }

    /// The rate those frames are counted at, in frames per second.
    ///
    /// Anything timing against this clock takes its rate from here rather than
    /// from a profile, since the two differ whenever the device granted
    /// something other than what was asked for.
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
