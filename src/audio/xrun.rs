//! Counting the blocks that were lost, from the audio callback to the
//! application thread.
//!
//! An xrun is the one thing the callback has to say that nothing else can say
//! for it: a block arrived that there was no room for, or a block was wanted
//! that was not there. Neither leaves a trace anywhere else — the samples are
//! simply gone — so a dropout is invisible unless the callback counts it as it
//! happens. Incrementing an atomic is one of the few things it may do.
//!
//! The two directions are counted apart because they mean opposite things. An
//! overrun says the application thread is not draining what the device
//! delivers; an underrun says it is not filling what the device asks for.
//! Summed into one number they cancel out into a count that names no fault at
//! all.
//!
//! Each count is a plain [`AtomicUsize`] rather than a pair packed into one
//! word, which is what [`level_meter`](super::level_meter) does and for a
//! reason that does not carry here. Peak and RMS read separately can produce a
//! pair no block ever had; two counts that only ever grow cannot. A reader that
//! catches one an instant before the other sees a number that is briefly stale,
//! never one that is impossible.
//!
//! Each count also has exactly one writer, which is why incrementing is a load
//! and a store rather than a read-modify-write: overruns are seen by the thread
//! capturing input, underruns by the thread playing output, and the two ends
//! are separate types so that neither can be handed to the wrong callback.
//!
//! Neither count handles wrapping, for the reason [`sample_ring`](super::sample_ring)
//! gives for its own: a 64-bit count has more room than the hardware has life,
//! and this one advances once per lost block rather than once per sample.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many blocks have been lost in each direction since counting began.
///
/// Both only ever grow. A caller watching for new dropouts subtracts a reading
/// it took earlier rather than expecting these to return to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Xruns {
    /// Blocks the output could not fill, because the samples to fill them with
    /// had not been produced yet.
    ///
    /// This is the one a player hears, as a gap in the audio.
    pub underruns: usize,
    /// Blocks of input that were dropped, because there was nowhere to put
    /// them.
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
/// overruns.overran();
/// underruns.underran();
/// underruns.underran();
///
/// assert_eq!(reader.read().overruns, 1);
/// assert_eq!(reader.read().underruns, 2);
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
    /// Count one block of input that was dropped.
    ///
    /// A block rather than a sample: what a caller wants to know is how many
    /// times the path failed, and a count of samples reports a long block as a
    /// worse failure than a short one when both lost the same moment of audio.
    pub fn overran(&mut self) {
        increment(&self.counts.overruns);
    }
}

/// The end that counts unfilled output, held by whichever thread plays it.
///
/// This is the end the output callback holds.
pub struct UnderrunCounter {
    counts: Arc<Counts>,
}

impl UnderrunCounter {
    /// Count one block of output that could not be filled.
    ///
    /// A block rather than a sample, for the reason given on
    /// [`OverrunCounter::overran`].
    pub fn underran(&mut self) {
        increment(&self.counts.underruns);
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
