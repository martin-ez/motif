//! The beats themselves, and the two views over them: where a frame falls, and
//! how fast they are going.
//!
//! Reading a grid indexes a slice and does arithmetic — no allocation, no lock,
//! and a search that halves rather than one that walks — so a consumer on the
//! real-time thread can read one (invariant 2). Growing a grid by a beat may
//! allocate, and belongs on the thread that took the timestamp.

/// The beats of a loop, as the frames they fall on.
///
/// Beats are frame indices against the sample clock they were timed by, which
/// the grid keeps so a tempo can be worked out from them. That is a clock, not a
/// tempo: [`beats_per_minute`](Self::beats_per_minute) recomputes it from the
/// timestamps every time it is asked.
///
/// Timestamps strictly increase — [`push`](Self::push) refuses one that does not
/// — so the interval a tempo is derived from is never empty.
///
/// ```
/// use motif::seq::{BeatGrid, Position};
///
/// let mut grid = BeatGrid::new(48_000);
/// for beat in [0, 24_000, 48_000] {
///     assert!(grid.push(beat));
/// }
///
/// assert_eq!(grid.beats_per_minute(), Some(120.0));
/// assert_eq!(
///     grid.position(36_000),
///     Position::Within { beat: 1, phase: 0.5 }
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatGrid {
    sample_rate: u32,
    beats: Vec<u64>,
}

impl BeatGrid {
    /// An empty grid over the clock `sample_rate` counts, in frames per second.
    ///
    /// The rate is the one the beats pushed onto it are timestamped against.
    /// A grid whose beats came from one clock and whose rate came from another
    /// reports a tempo that is wrong by their ratio.
    pub const fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            beats: Vec::new(),
        }
    }

    /// Add a beat at frame `at`, reporting whether it was added.
    ///
    /// A timestamp that does not come after the last beat is not one: it is
    /// refused and the grid is left alone. That is a stale reading to drop
    /// rather than a failure to report, so it is a `bool` and not an error —
    /// the caller that cares tries again with a later timestamp, and the caller
    /// that does not is no worse off.
    ///
    /// This may allocate, so it does not belong on the audio callback.
    ///
    /// ```
    /// use motif::seq::BeatGrid;
    ///
    /// let mut grid = BeatGrid::new(48_000);
    ///
    /// assert!(grid.push(24_000));
    /// assert!(!grid.push(24_000));
    /// assert!(!grid.push(12_000));
    /// assert_eq!(grid.beats(), &[24_000]);
    /// ```
    #[must_use]
    pub fn push(&mut self, at: u64) -> bool {
        if self.beats.last().is_some_and(|&last| at <= last) {
            return false;
        }

        self.beats.push(at);
        true
    }

    /// Every beat, in the order they fall.
    pub fn beats(&self) -> &[u64] {
        &self.beats
    }

    /// How many beats the grid holds.
    pub fn len(&self) -> usize {
        self.beats.len()
    }

    /// Whether no beat has been added yet.
    pub fn is_empty(&self) -> bool {
        self.beats.is_empty()
    }

    /// The clock the beats are timestamped against, in frames per second.
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The tempo the beats describe, averaged over the whole grid.
    ///
    /// Derived on every call from the span the beats cover and how many
    /// intervals divide it, so a grid that speeds up or slows down says so
    /// without anything being updated.
    ///
    /// The average is over the whole grid rather than its last interval, which
    /// keeps it steady under a beat that lands early or late.
    ///
    /// `None` below two beats, which describe no interval, and for a grid with
    /// no sample rate, whose frames are not a duration.
    pub fn beats_per_minute(&self) -> Option<f64> {
        let intervals = self.beats.len().checked_sub(1).filter(|&count| count > 0)?;
        if self.sample_rate == 0 {
            return None;
        }

        let span = self.beats.last()? - self.beats.first()?;

        Some(SECONDS_PER_MINUTE * f64::from(self.sample_rate) * intervals as f64 / span as f64)
    }

    /// Where frame `frame` falls against the beats.
    ///
    /// Reads a slice and halves its search, so this allocates nothing, blocks
    /// on nothing and is bounded by the size of the grid.
    pub fn position(&self, frame: u64) -> Position {
        let following = self.beats.partition_point(|&beat| beat <= frame);

        let Some(beat) = following.checked_sub(1) else {
            return Position::BeforeFirst;
        };
        let Some(&next) = self.beats.get(following) else {
            return Position::AfterLast { beat };
        };

        let started = self.beats[beat];
        Position::Within {
            beat,
            phase: (frame - started) as f64 / (next - started) as f64,
        }
    }
}

const SECONDS_PER_MINUTE: f64 = 60.0;

/// Where a frame falls against a [`BeatGrid`].
///
/// The three cases are the three a grid of timestamps can answer. A grid holds
/// beats that have already happened, so the frame after the last one is placed
/// on that beat rather than projected onto the next: how far through a beat
/// something is needs the beat it ends on, and that beat has not been played
/// yet.
///
/// An empty grid places every frame at [`Position::BeforeFirst`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Position {
    /// Before the first beat, which includes every frame of an empty grid.
    BeforeFirst,
    /// Between two beats.
    Within {
        /// The beat it is on or after, indexed into [`BeatGrid::beats`].
        beat: usize,
        /// How far it sits through the interval to the next beat, from `0.0`
        /// on the beat up to but not reaching `1.0` on the next.
        phase: f64,
    },
    /// On or after the last beat, whose interval has no end yet.
    AfterLast {
        /// The last beat, indexed into [`BeatGrid::beats`].
        beat: usize,
    },
}
