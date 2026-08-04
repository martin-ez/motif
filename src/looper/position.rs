//! Publishing where the loop has got to, from the audio callback to the thread
//! that draws it.
//!
//! A playhead is not a queue. The thread drawing it wants where the loop is now,
//! not every position since it last looked, so a position published while nobody
//! was reading is overwritten rather than kept. That makes the whole crossing one
//! atomic store against one atomic load: wait-free at both ends, and a fixed cost
//! per block on the end that may not wait.
//!
//! The playhead and the length travel packed into a single [`AtomicU64`] rather
//! than in two atomics side by side. Read separately they can straddle two
//! blocks, and the pair that comes back is then one no block ever had — a
//! playhead beyond the end of the loop it sits in, which a bar would draw past
//! its own end. Packed, there is nothing to straddle. Two frame counts fill the
//! word exactly, so nothing is stolen from either to tag it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const LENGTH_BITS: u32 = 32;

/// Where the loop has got to, and how long it is.
///
/// Both are frame counts, so turning either into a time is the reader's own
/// affair: frames are what the callback counts, and the sample rate that
/// converts them belongs to the device rather than to the meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopPosition {
    playhead: u32,
    recorded: u32,
}

impl LoopPosition {
    /// A loop with nothing recorded and nowhere to be.
    pub const EMPTY: Self = Self {
        playhead: 0,
        recorded: 0,
    };

    /// A playhead `playhead` frames into a loop of `recorded` frames.
    ///
    /// A playhead past the end is held at the end rather than kept as given. The
    /// pair is published as one word so that a reader never sees a position
    /// outside its own loop, and a constructor that let one be built would put
    /// that back by the front door.
    ///
    /// ```
    /// use motif::looper::LoopPosition;
    ///
    /// let past_the_end = LoopPosition::new(9_000, 4_000);
    ///
    /// assert_eq!(past_the_end.playhead(), 4_000);
    /// ```
    pub const fn new(playhead: u32, recorded: u32) -> Self {
        Self {
            playhead: if playhead > recorded {
                recorded
            } else {
                playhead
            },
            recorded,
        }
    }

    /// How many frames into the loop the playhead is.
    pub const fn playhead(self) -> u32 {
        self.playhead
    }

    /// How many frames long the loop is.
    pub const fn recorded(self) -> u32 {
        self.recorded
    }

    const fn packed(self) -> u64 {
        ((self.playhead as u64) << LENGTH_BITS) | self.recorded as u64
    }

    const fn unpacked(packed: u64) -> Self {
        Self::new((packed >> LENGTH_BITS) as u32, packed as u32)
    }
}

/// Build a playhead meter, and split it into the end that publishes and the end
/// that reads.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// use motif::looper::{LoopPosition, position_meter};
///
/// let (mut writer, reader) = position_meter();
///
/// writer.publish(LoopPosition::new(24_000, 96_000));
///
/// assert_eq!(reader.read().playhead(), 24_000);
/// ```
pub fn position_meter() -> (PositionWriter, PositionReader) {
    let published = Arc::new(AtomicU64::new(LoopPosition::EMPTY.packed()));

    (
        PositionWriter {
            published: Arc::clone(&published),
        },
        PositionReader { published },
    )
}

/// The publishing end of a playhead meter, held by whichever thread moves the
/// loop along.
///
/// This is the end the audio callback holds.
pub struct PositionWriter {
    published: Arc<AtomicU64>,
}

impl PositionWriter {
    /// Publish `position`, replacing whatever was there.
    ///
    /// A position the reader never looked at is gone, which is the whole of what
    /// makes this safe to call from a callback that cannot wait for a reader.
    pub fn publish(&mut self, position: LoopPosition) {
        self.published.store(position.packed(), Ordering::Release);
    }
}

/// The reading end of a playhead meter, held by whichever thread draws it.
///
/// This is the end the application thread holds.
pub struct PositionReader {
    published: Arc<AtomicU64>,
}

impl PositionReader {
    /// The most recently published position, or [`LoopPosition::EMPTY`] where
    /// none has been published yet.
    ///
    /// Reading takes nothing: the same position reads the same way until the
    /// next one replaces it, so a screen running faster than the audio callback
    /// repeats a position rather than finding nothing there.
    pub fn read(&self) -> LoopPosition {
        LoopPosition::unpacked(self.published.load(Ordering::Acquire))
    }
}
