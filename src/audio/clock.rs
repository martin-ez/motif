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
//! deadline depends on (invariant 2).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Build a sample clock, and split it into the end that counts and the end that
/// reads.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// use motif::audio::sample_clock;
///
/// let (mut elapsed, now) = sample_clock();
///
/// elapsed.advance(128);
/// elapsed.advance(128);
///
/// assert_eq!(now.read(), 256);
/// ```
pub fn sample_clock() -> (SampleClockWriter, SampleClockReader) {
    let counted = Arc::new(AtomicU64::new(0));

    (
        SampleClockWriter {
            counted: Arc::clone(&counted),
        },
        SampleClockReader { counted },
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

/// The reading end of a sample clock, held by whichever thread timestamps
/// against it.
pub struct SampleClockReader {
    counted: Arc<AtomicU64>,
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
}
