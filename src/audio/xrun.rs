//! Counting the callbacks that lost frames, from the audio callback to the
//! application thread.
//!
//! An xrun is the one thing the callback has to say that nothing else can say
//! for it: frames arrived that there was no room for, or frames were wanted
//! that were not there. Neither leaves a trace anywhere else — the samples are
//! simply gone — so a dropout is invisible unless the callback counts it as it
//! happens. Incrementing an atomic is one of the few things it may do.
//!
//! The two directions are counted apart because they name opposite faults. An
//! overrun says whatever drains the path is not keeping up with the device
//! delivering into it; an underrun says whatever fills the path is not keeping
//! up with the device asking of it. Which threads those are is the caller's
//! arrangement — under [`passthrough`](super::passthrough) both are callbacks,
//! and no application thread is involved at all. Summed into one number the two
//! cancel into a count that names no fault.
//!
//! Each count is a plain [`AtomicUsize`] rather than a pair packed into one
//! word, which is what [`level_meter`](super::level_meter) does and for a
//! reason that does not carry here. Peak and RMS read separately can produce a
//! pair no block ever had; two counts that only ever grow cannot. A reader that
//! catches one an instant before the other sees a number that is briefly stale,
//! never one that is impossible.
//!
//! They do share a cache line, and are left sharing it. A store happens only
//! when a dropout does, so between dropouts the line is read-only; dropouts
//! frequent enough for two writers to contend over it are already the failure
//! the count exists to report, and padding would buy nothing on the path that
//! matters.
//!
//! Each count also has exactly one writer, which is why incrementing is a load
//! and a store rather than a read-modify-write: overruns are seen where input
//! is captured, underruns where output is played, and the two ends are separate
//! types so that neither can be handed to the wrong callback.
//!
//! Neither count handles wrapping, for the reason [`sample_ring`](super::sample_ring)
//! gives for its own: a 64-bit count has more room than the hardware has life,
//! and this one advances once per failed callback rather than once per sample.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many callbacks have lost frames in each direction since counting began.
///
/// A callback that lost one frame and a callback that lost every frame it had
/// count the same, because what these answer is how often the path failed.
///
/// Both only ever grow. A caller watching for new dropouts subtracts a reading
/// it took earlier rather than expecting these to return to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Xruns {
    /// Callbacks that could not fill the output they were asked for, because
    /// the samples to fill it with had not been produced yet.
    ///
    /// This is the one a player hears, as a gap in the audio.
    pub underruns: usize,
    /// Callbacks that had input dropped, because there was nowhere to put it.
    ///
    /// This one is silent, and shows up as audio that was played but never
    /// captured.
    pub overruns: usize,
}

impl Xruns {
    /// Nothing lost in either direction.
    pub const NONE: Self = Self {
        underruns: 0,
        overruns: 0,
    };
}

/// Build a counter, and split it into the end that counts dropped input, the
/// end that counts unfilled output, and the end that reads both.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
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

/// The end that counts dropped input, held by whichever thread captures it.
///
/// This is the end the input callback holds.
pub struct OverrunCounter {
    counts: Arc<Counts>,
}

impl OverrunCounter {
    /// Count a callback that took `captured` frames of the `offered` it was
    /// handed, which is an overrun when it took fewer.
    ///
    /// Deciding here rather than at the call site is what makes the rule
    /// testable: the callback that would otherwise hold it cannot be built
    /// without a device.
    ///
    /// A callback rather than a frame. What a caller wants to know is how often
    /// the path failed, and counting frames reports a long block as a worse
    /// failure than a short one when both lost the same moment of audio.
    pub fn captured(&mut self, captured: usize, offered: usize) {
        if captured < offered {
            increment(&self.counts.overruns);
        }
    }
}

/// The end that counts unfilled output, held by whichever thread plays it.
///
/// This is the end the output callback holds.
pub struct UnderrunCounter {
    counts: Arc<Counts>,
}

impl UnderrunCounter {
    /// Count a callback that filled `supplied` frames of the `wanted` it was
    /// asked for, which is an underrun when it filled fewer.
    ///
    /// A callback rather than a frame, and decided here rather than at the call
    /// site, for the reasons given on [`OverrunCounter::captured`].
    pub fn supplied(&mut self, supplied: usize, wanted: usize) {
        if supplied < wanted {
            increment(&self.counts.underruns);
        }
    }
}

/// The reading end of a counter, held by whichever thread reports it.
///
/// This is the end the application thread holds.
pub struct XrunReader {
    counts: Arc<Counts>,
}

impl XrunReader {
    /// What has been counted so far.
    ///
    /// Reading takes nothing and resets nothing: the same dropouts read the
    /// same way until another one happens, so two readings that differ are two
    /// readings with a real dropout between them.
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
