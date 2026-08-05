//! Publishing where the loop has got to, from the audio callback to the thread
//! that draws it.
//!
//! A playhead is not a queue. The thread drawing it wants where the loop is now,
//! not every position since it last looked, so a position published while nobody
//! was reading is overwritten rather than kept. That makes the whole crossing one
//! atomic store against one atomic load: wait-free at both ends, and a fixed cost
//! per block on the end that may not wait.
//!
//! The playhead, the length and the depth travel packed into a single
//! [`AtomicU64`]. Read as separate atomics they can straddle two blocks and come
//! back as a set no block ever had — a playhead beyond the end of its own loop,
//! or a depth of layers over a loop that was just cleared. Two frame counts and
//! a depth fill the word exactly, so nothing is stolen from any of them to tag
//! it, and the whole crossing stays one store against one load.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::LoopBuffer;

const FRAME_BITS: u32 = 30;
const DEPTH_BITS: u32 = 4;
const LENGTH_SHIFT: u32 = DEPTH_BITS;
const PLAYHEAD_SHIFT: u32 = FRAME_BITS + DEPTH_BITS;
const FRAME_MASK: u64 = (1 << FRAME_BITS) - 1;
const DEPTH_MASK: u64 = (1 << DEPTH_BITS) - 1;

/// Where the loop has got to, how long it is, and how many layers it is built
/// from.
///
/// The first two are frame counts, so turning either into a time is the
/// reader's own affair: frames are what the callback counts, and the sample
/// rate that converts them belongs to the device rather than to the meter. The
/// depth is layers, and what it is a fraction of is
/// [`LoopBuffer::LAYERS`](crate::looper::LoopBuffer::LAYERS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopPosition {
    playhead: u32,
    recorded: u32,
    depth: usize,
}

impl LoopPosition {
    /// A loop with nothing recorded, no layers, and nowhere to be.
    pub const EMPTY: Self = Self {
        playhead: 0,
        recorded: 0,
        depth: 0,
    };

    /// The longest loop a position can carry, in frames.
    ///
    /// Just over a billion frames, which is six hours at 48 kHz against the
    /// thirty-two seconds
    /// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET) records
    /// into. What the ceiling buys is the room the depth rides in: the word
    /// holds two frame counts and a depth, and a frame count wide enough to
    /// count hours nobody can record is the cheapest of the three to shorten.
    pub const MAX_FRAMES: u32 = FRAME_MASK as u32;

    /// A playhead `playhead` frames into a loop of `recorded` frames, built from
    /// `depth` layers.
    ///
    /// A playhead past the end is held at the end, a frame count past
    /// [`MAX_FRAMES`](Self::MAX_FRAMES) at that, and a depth past
    /// [`LoopBuffer::LAYERS`](crate::looper::LoopBuffer::LAYERS) at that. The
    /// three are published as one word so that a reader never sees a position
    /// outside its own loop, and a constructor that let one be built would put
    /// that back by the front door.
    ///
    /// ```
    /// use motif::looper::LoopPosition;
    ///
    /// let past_the_end = LoopPosition::new(9_000, 4_000, 1);
    ///
    /// assert_eq!(past_the_end.playhead(), 4_000);
    /// ```
    pub fn new(playhead: u32, recorded: u32, depth: usize) -> Self {
        let recorded = recorded.min(Self::MAX_FRAMES);

        Self {
            playhead: playhead.min(recorded),
            recorded,
            depth: depth.min(LoopBuffer::LAYERS),
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

    /// How many layers hold audio, from none up to
    /// [`LoopBuffer::LAYERS`](crate::looper::LoopBuffer::LAYERS).
    pub const fn depth(self) -> usize {
        self.depth
    }

    fn packed(self) -> u64 {
        ((self.playhead as u64) << PLAYHEAD_SHIFT)
            | ((self.recorded as u64) << LENGTH_SHIFT)
            | self.depth as u64
    }

    fn unpacked(packed: u64) -> Self {
        Self::new(
            (packed >> PLAYHEAD_SHIFT) as u32,
            ((packed >> LENGTH_SHIFT) & FRAME_MASK) as u32,
            (packed & DEPTH_MASK) as usize,
        )
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
/// writer.publish(LoopPosition::new(24_000, 96_000, 2));
///
/// assert_eq!(reader.read().playhead(), 24_000);
/// assert_eq!(reader.read().depth(), 2);
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
