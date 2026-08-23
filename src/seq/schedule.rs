//! Handing the beats that have not happened yet to the thread that plays them.
//!
//! The other direction from [`position_meter`](crate::looper::position_meter):
//! the grid is grown on the thread that timestamps a tap, and the click is
//! sounded on the thread that may not allocate, so the beats have to cross. What
//! crosses is timestamps and only timestamps — a tempo reconstructed on the far
//! side would be invariant 3's failure with a thread boundary in front of it.
//!
//! A window rather than a queue, so a player who restates a tempo replaces what
//! was coming rather than clicking it out. The count is what publishes it, and a
//! window read while one is being written can hold beats from both — every one
//! of them is still a beat something projected, so the worst that costs is a
//! click early or a click missed, once, where the tempo changed.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::BeatGrid;

struct Shared {
    beats: [AtomicU64; BeatsAhead::BEATS],
    count: AtomicUsize,
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
        beats: [const { AtomicU64::new(0) }; BeatsAhead::BEATS],
        count: AtomicUsize::new(0),
    });

    (
        ScheduleWriter {
            shared: Arc::clone(&shared),
        },
        ScheduleReader { shared },
    )
}

/// The end of a schedule that follows a grid, held by the thread that grew it.
pub struct ScheduleWriter {
    shared: Arc<Shared>,
}

impl ScheduleWriter {
    /// Schedule the beats `grid` puts after frame `after`, replacing whatever
    /// was scheduled before.
    ///
    /// Projected by [`BeatGrid::next_beat`], so a grid that has not reached the
    /// frame yet still schedules beats and one with no interval to project by
    /// schedules none. The beats are written before the count that publishes
    /// them, which is what puts them in front of a reader that finds it.
    pub fn follow(&mut self, grid: &BeatGrid, after: u64) {
        let mut beat = after;
        let mut count = 0;

        for slot in &self.shared.beats {
            let Some(next) = grid.next_beat(beat) else {
                break;
            };

            slot.store(next, Ordering::Relaxed);
            beat = next;
            count += 1;
        }

        self.shared.count.store(count, Ordering::Release);
    }

    /// Take every beat off the schedule, so that nothing is left to sound.
    ///
    /// What the player withdrawing a tempo means: the beats that were coming
    /// are not, and a window already published would otherwise go on being
    /// read.
    pub fn silence(&mut self) {
        self.shared.count.store(0, Ordering::Release);
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
    /// no more than a full window on any block.
    pub fn read(&self) -> BeatsAhead {
        let published = self.shared.count.load(Ordering::Acquire);
        let mut beats = [0; BeatsAhead::BEATS];
        let mut count = 0;

        for (beat, slot) in beats.iter_mut().zip(&self.shared.beats).take(published) {
            *beat = slot.load(Ordering::Relaxed);
            count += 1;
        }

        BeatsAhead { beats, count }
    }
}
