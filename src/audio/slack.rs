//! Holding the frames a boundary keeps between capture and playback, against
//! two clocks that will not agree.
//!
//! The playback end starts a block behind the capture end, and that gap is the
//! whole of the path's give: an underrun spends it once, and drift between two
//! devices spends it steadily. Nothing here resamples, so the correction is
//! frames — a few given up where the ring runs long, a few played that it never
//! supplied where it runs short, capped per block so that holding the slack
//! costs a discontinuity rather than a gap.
//!
//! What it cost is three atomics with one writer, stored with a load and a
//! store as [`xrun_counter`](super::xrun_counter)'s counts are.

use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as Atomic};

/// Build the hold for a boundary keeping `slack` frames between its ends and
/// running `block` frames to a callback, and split it into the end that trims
/// and the end that reads.
///
/// Allocates here and never again, so this belongs in setup, before the stream
/// starts.
///
/// ```
/// use motif::audio::{Trim, slack_hold};
///
/// let (mut trim, reader) = slack_hold(256, 256);
///
/// assert_eq!(trim.trim(512, 256), Trim::Steady);
/// assert_eq!(reader.read().held, 256);
/// ```
pub fn slack_hold(slack: usize, block: usize) -> (SlackTrim, SlackReader) {
    let spent = Arc::new(Spent {
        held: AtomicUsize::new(0),
        dropped: AtomicUsize::new(0),
        inserted: AtomicUsize::new(0),
    });

    (
        SlackTrim {
            slack,
            block,
            wander: wander(block),
            most: most_per_block(block),
            correcting: false,
            spent: Arc::clone(&spent),
        },
        SlackReader { spent },
    )
}

/// What the playback end does to a block to hold the slack.
///
/// The counts are small by construction, so a correction lands as a
/// discontinuity a few samples wide rather than as silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trim {
    /// Play the block as the ring gives it.
    Steady,
    /// Take this many frames off the ring without playing them.
    Drop(usize),
    /// Play this many frames the ring did not supply, holding the last one it
    /// did.
    Insert(usize),
}

/// The trimming end, held by the playback callback.
///
/// A slow integrator with a dead band: nothing happens while the ring is within
/// half a block of where it should be, and once it is not, a few frames a
/// callback go until the ring is back on target rather than back inside the
/// band. Half a block is what two callbacks that are not interleaved wander by
/// on their own, and correcting only to the edge of that would leave every
/// excursion permanently paid for out of the slack.
pub struct SlackTrim {
    slack: usize,
    block: usize,
    wander: usize,
    most: usize,
    correcting: bool,
    spent: Arc<Spent>,
}

impl SlackTrim {
    /// What to do with a block of `wanted` frames, against a ring holding
    /// `available`.
    ///
    /// Publishes what it decided as it decides it, so the reading end sees
    /// every block. A ring too dry to fill the block even after an insert is
    /// left alone: the shortfall it is already reporting restores the slack
    /// faster than an insert would, and the block comes up short either way.
    ///
    /// Runs on the audio thread, so it neither allocates nor locks.
    pub fn trim(&mut self, available: usize, wanted: usize) -> Trim {
        let target = self.slack + wanted;
        self.correcting |= available.abs_diff(target) > self.wander;

        let trim = match (self.correcting, available.cmp(&target)) {
            (false, _) => Trim::Steady,
            (true, Ordering::Greater) => Trim::Drop(self.most.min(available - target)),
            (true, Ordering::Less) => self.padding(available, wanted, target - available),
            (true, Ordering::Equal) => {
                self.correcting = false;
                Trim::Steady
            }
        };

        self.publish(trim, available, wanted);
        trim
    }

    fn padding(&self, available: usize, wanted: usize, missing: usize) -> Trim {
        let frames = missing.min(self.most).min(room_to_hold(self.block, wanted));

        if frames == 0 || available + frames < wanted {
            return Trim::Steady;
        }
        Trim::Insert(frames)
    }

    fn publish(&self, trim: Trim, available: usize, wanted: usize) {
        let (dropped, inserted) = match trim {
            Trim::Steady => (0, 0),
            Trim::Drop(frames) => (frames, 0),
            Trim::Insert(frames) => (0, frames),
        };

        self.spent.held.store(
            available.saturating_sub(dropped + wanted - inserted),
            Atomic::Release,
        );
        add(&self.spent.dropped, dropped);
        add(&self.spent.inserted, inserted);
    }
}

/// The slack a boundary is holding, and what holding it has cost.
///
/// The two counts only ever grow, so a caller after what the last few seconds
/// cost subtracts an earlier reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slack {
    /// Frames left between the two ends once the last block was played.
    pub held: usize,
    /// Frames given up to bring the slack down.
    pub dropped: usize,
    /// Frames played that the ring never supplied, to bring the slack up.
    pub inserted: usize,
}

impl Slack {
    /// Nothing held and nothing spent, which is what a boundary reports before
    /// its first block and a stream with no ring behind it reports always.
    pub const NONE: Self = Self {
        held: 0,
        dropped: 0,
        inserted: 0,
    };
}

/// The reading end, held by whichever thread reports what the stream is doing.
#[derive(Clone)]
pub struct SlackReader {
    spent: Arc<Spent>,
}

impl SlackReader {
    /// What the trim has been holding, and what it spent getting there.
    pub fn read(&self) -> Slack {
        Slack {
            held: self.spent.held.load(Atomic::Acquire),
            dropped: self.spent.dropped.load(Atomic::Acquire),
            inserted: self.spent.inserted.load(Atomic::Acquire),
        }
    }
}

struct Spent {
    held: AtomicUsize,
    dropped: AtomicUsize,
    inserted: AtomicUsize,
}

fn wander(block: usize) -> usize {
    (block / 2).max(1)
}

fn most_per_block(block: usize) -> usize {
    (block / 32).max(1)
}

fn room_to_hold(block: usize, wanted: usize) -> usize {
    block.min(wanted).saturating_sub(1)
}

fn add(count: &AtomicUsize, frames: usize) {
    if frames == 0 {
        return;
    }
    count.store(count.load(Atomic::Relaxed) + frames, Atomic::Release);
}
