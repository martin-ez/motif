//! Counting the callbacks that lost frames, and reading the count elsewhere.
//!
//! A dropout leaves no other trace — the samples are simply gone — so nothing
//! downstream can tell they were ever due. The two directions stay apart
//! because they name opposite faults, and summed they would cancel.
//!
//! Each count has one writer, which is what lets a callback increment with a
//! load and a store rather than a read-modify-write. They are two atomics
//! rather than the word [`level_meter`](super::level_meter) packs its pair
//! into: that packing prevents a pair no block ever had, and counts that only
//! ever grow have none to prevent. The cache line they share is left shared,
//! since a store happens only when a dropout does.
//!
//! Wrapping goes unhandled, as it does in [`sample_ring`](super::sample_ring).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many callbacks have lost frames in each direction since counting began.
///
/// A callback that lost one frame counts the same as one that lost every frame:
/// these answer how often the path failed. Both only ever grow, so a caller
/// after recent dropouts subtracts an earlier reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Xruns {
    /// Callbacks that could not fill the output asked of them.
    ///
    /// This is the one a player hears, as a gap in the audio.
    pub underruns: usize,
    /// Callbacks that had input dropped for want of anywhere to put it.
    ///
    /// This one is silent: audio that was played but never captured.
    pub overruns: usize,
}

impl Xruns {
    /// Nothing lost in either direction.
    pub const NONE: Self = Self {
        underruns: 0,
        overruns: 0,
    };
}

/// Build a counter, and split it into the two counting ends and the reading
/// end.
///
/// Allocates here and never again, so this belongs in setup, before the stream
/// starts.
///
/// ```
/// let (mut overruns, mut underruns, reader) = motif::audio::xrun_counter();
///
/// overruns.captured(64, 64);
/// overruns.captured(48, 64);
/// underruns.supplied(64, 64);
///
/// assert_eq!(reader.read().overruns, 1);
/// assert_eq!(reader.read().underruns, 0);
/// ```
pub fn xrun_counter() -> (OverrunCounter, UnderrunCounter, XrunReader) {
    let counts = Arc::new(Counts {
        underruns: AtomicUsize::new(0),
        overruns: AtomicUsize::new(0),
    });

    (
        OverrunCounter {
            counts: Arc::clone(&counts),
        },
        UnderrunCounter {
            counts: Arc::clone(&counts),
        },
        XrunReader { counts },
    )
}

/// The counting end held where input is captured.
pub struct OverrunCounter {
    counts: Arc<Counts>,
}

impl OverrunCounter {
    /// Count an overrun when `captured` falls short of the `offered` frames the
    /// callback was handed.
    ///
    /// The comparison lives here so that a test can reach it: the callback that
    /// would otherwise hold it needs a device.
    pub fn captured(&mut self, captured: usize, offered: usize) {
        if captured < offered {
            increment(&self.counts.overruns);
        }
    }
}

/// The counting end held where output is played.
pub struct UnderrunCounter {
    counts: Arc<Counts>,
}

impl UnderrunCounter {
    /// Count an underrun when `supplied` falls short of the `wanted` frames the
    /// callback was asked for.
    pub fn supplied(&mut self, supplied: usize, wanted: usize) {
        if supplied < wanted {
            increment(&self.counts.underruns);
        }
    }
}

/// The reading end, held by whichever thread reports the count.
pub struct XrunReader {
    counts: Arc<Counts>,
}

impl XrunReader {
    /// What has been counted so far.
    ///
    /// Resets nothing, so two readings that differ have a real dropout between
    /// them.
    pub fn read(&self) -> Xruns {
        Xruns {
            underruns: self.counts.underruns.load(Ordering::Acquire),
            overruns: self.counts.overruns.load(Ordering::Acquire),
        }
    }
}

struct Counts {
    underruns: AtomicUsize,
    overruns: AtomicUsize,
}

fn increment(count: &AtomicUsize) {
    count.store(count.load(Ordering::Relaxed) + 1, Ordering::Release);
}
