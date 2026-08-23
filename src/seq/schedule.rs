//! Handing the beats that have not happened yet to the thread that plays them.
//!
//! The other direction from [`position_meter`](crate::looper::position_meter):
//! the grid is grown on the thread that timestamps a tap, and the click is
//! sounded on the thread that may not allocate, so the beats have to cross. What
//! crosses is timestamps and only timestamps — a tempo reconstructed on the far
//! side would be invariant 3's failure with a thread boundary in front of it.
//!
//! A window rather than a queue. A queue would go on clicking a tempo the player
//! had already abandoned, where a window is replaced whole by the next one
//! published; and a window of several beats rather than one means a redraw that
//! comes late costs no click. Reading one is a fixed number of relaxed loads
//! against a slot nothing is writing, which is what the callback may spend.

use std::array;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::BeatGrid;

const SLOT_COUNT: usize = 3;

struct Shared {
    slots: [[AtomicU64; BeatsAhead::BEATS]; SLOT_COUNT],
    counts: [AtomicUsize; SLOT_COUNT],
    published: AtomicUsize,
}

/// The beats a schedule is holding, as the frames they fall on.
///
/// Fixed size and [`Copy`], so the callback reading one takes a value rather
/// than a borrow it would have to hold across a block. The beats come back in
/// the order they fall, and every one of them is still ahead of the frame the
/// schedule was followed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeatsAhead {
    beats: [u64; Self::BEATS],
    count: usize,
}

impl BeatsAhead {
    /// How far ahead a schedule reaches, in beats.
    ///
    /// Eight, which is four seconds at the fastest pulse anyone taps and
    /// sixteen at the slowest: long enough that a redraw the machine was too
    /// busy to run costs no click, and short enough to read inside a block.
    pub const BEATS: usize = 8;

    /// The beats, in the order they fall.
    pub fn beats(&self) -> &[u64] {
        &self.beats[..self.count]
    }
}

/// Build a beat schedule, and split it into the end that follows a grid and the
/// end that reads what it found.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// use motif::seq::{BeatGrid, beat_schedule};
///
/// let (mut writer, reader) = beat_schedule();
/// let mut grid = BeatGrid::new(48_000);
/// for beat in [0, 24_000] {
///     assert!(grid.push(beat));
/// }
///
/// writer.follow(&grid, 24_000);
///
/// assert_eq!(reader.read().beats()[0], 48_000);
/// ```
pub fn beat_schedule() -> (ScheduleWriter, ScheduleReader) {
    let shared = Arc::new(Shared {
        slots: array::from_fn(|_| array::from_fn(|_| AtomicU64::new(0))),
        counts: [const { AtomicUsize::new(0) }; SLOT_COUNT],
        published: AtomicUsize::new(0),
    });

    (
        ScheduleWriter {
            shared: Arc::clone(&shared),
            filling: 0,
        },
        ScheduleReader { shared },
    )
}

/// The end of a schedule that follows a grid, held by the thread that grew it.
pub struct ScheduleWriter {
    shared: Arc<Shared>,
    filling: usize,
}

impl ScheduleWriter {
    /// Schedule the beats `grid` puts after frame `after`, replacing whatever
    /// was scheduled before.
    ///
    /// Projected by [`BeatGrid::next_beat`], so a grid that has not reached the
    /// frame yet still schedules beats and one stating no tempo schedules none.
    /// It fills a slot the reader is not reading and publishes it afterwards,
    /// which is what keeps a window from being read half replaced.
    pub fn follow(&mut self, grid: &BeatGrid, after: u64) {
        let mut count = 0;
        let mut beat = after;
        while count < BeatsAhead::BEATS {
            let Some(next) = grid.next_beat(beat) else {
                break;
            };

            self.shared.slots[self.filling][count].store(next, Ordering::Relaxed);
            beat = next;
            count += 1;
        }

        self.publish(count);
    }

    /// Take every beat off the schedule, so that nothing is left to sound.
    ///
    /// What the player withdrawing a tempo means: the beats that were coming
    /// are not, and a window already published would otherwise go on being
    /// read.
    pub fn silence(&mut self) {
        self.publish(0);
    }

    fn publish(&mut self, count: usize) {
        self.shared.counts[self.filling].store(count, Ordering::Relaxed);
        self.shared.published.store(self.filling, Ordering::Release);
        self.filling = (self.filling + 1) % SLOT_COUNT;
    }
}

/// The end of a schedule that reads the beats, held by the audio callback.
pub struct ScheduleReader {
    shared: Arc<Shared>,
}

impl ScheduleReader {
    /// The beats scheduled as of the last window published.
    ///
    /// One acquiring load and a bounded run of relaxed ones into a value that
    /// is already sized, so this allocates nothing, blocks on nothing and costs
    /// the same on every block.
    pub fn read(&self) -> BeatsAhead {
        let published = self.shared.published.load(Ordering::Acquire);
        let count = self.shared.counts[published].load(Ordering::Relaxed);

        BeatsAhead {
            beats: array::from_fn(|beat| {
                self.shared.slots[published][beat].load(Ordering::Relaxed)
            }),
            count: count.min(BeatsAhead::BEATS),
        }
    }
}
