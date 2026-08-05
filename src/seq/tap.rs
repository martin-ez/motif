//! Stating a tempo by tapping it, without typing a number.
//!
//! The taps are the grid (invariant 3): each one is kept as the frame it
//! arrived on and the tempo is read back off them, so a player who taps a
//! little unevenly has stated exactly that rather than an average of it.
//!
//! Which taps belong together is the whole of the work here. A sequence is a
//! run of taps at a comparable interval, and anything that does not resemble
//! one starts a fresh sequence rather than being folded into the old one: a
//! player who fumbles or comes back later taps again, and a tempo nobody is
//! still stating is not one to keep offering. What counts as resembling it is
//! a wide band, because it is telling a fumble from a fresh start rather than
//! smoothing the pulse — an uneven tap belongs on the grid.

use crate::seq::BeatGrid;

/// A pulse a player is tapping, and the grid it makes.
///
/// ```
/// use motif::seq::TapTempo;
///
/// let mut taps = TapTempo::new(48_000);
/// for tap in [0, 24_000, 48_000] {
///     taps.tap(tap);
/// }
///
/// assert_eq!(taps.tempo(), Some(120.0));
/// assert_eq!(taps.grid().beats(), &[0, 24_000, 48_000]);
/// ```
pub struct TapTempo {
    grid: BeatGrid,
}

impl TapTempo {
    /// How many taps a tempo is offered after.
    ///
    /// Two taps are one interval, which a single stray press is enough to
    /// make; three are a pulse the player has kept, and are what
    /// [`tempo`](Self::tempo) waits for.
    pub const TAPS_TO_A_TEMPO: usize = 3;

    /// How long a silence ends a sequence, in seconds.
    ///
    /// Two seconds is 30 BPM, slower than a pulse anyone taps, so a gap this
    /// long is a player who has stopped rather than one tapping very slowly.
    pub const STALE_AFTER_SECONDS: u64 = 2;

    /// Nothing tapped yet, against the clock `sample_rate` counts.
    ///
    /// The rate is the one taps are timestamped against, and the one the grid
    /// carries.
    pub const fn new(sample_rate: u32) -> Self {
        Self {
            grid: BeatGrid::new(sample_rate),
        }
    }

    /// Take a tap at frame `at`, reporting whether it joined the sequence
    /// rather than starting one.
    ///
    /// A tap starts a fresh sequence when it is the first, when it comes more
    /// than [`STALE_AFTER_SECONDS`](Self::STALE_AFTER_SECONDS) after the one
    /// before, or when its interval is less than half or more than double the
    /// sequence's average. A timestamp that does not come after the last tap
    /// is a stale reading of the clock and is dropped, leaving the sequence
    /// alone.
    pub fn tap(&mut self, at: u64) -> bool {
        let Some(&last) = self.grid.beats().last() else {
            return self.restart(at);
        };

        let Some(interval) = at.checked_sub(last).filter(|&interval| interval > 0) else {
            return false;
        };

        if interval > self.stale_after() || !self.resembles_the_sequence(interval) {
            return self.restart(at);
        }

        self.grid.push(at)
    }

    /// The taps, as the grid they make.
    ///
    /// Public because the grid is what the rest of the engine reads: a
    /// metronome or a quantiser works from the beats, not from the tapping.
    pub const fn grid(&self) -> &BeatGrid {
        &self.grid
    }

    /// The tempo the sequence states, or `None` below
    /// [`TAPS_TO_A_TEMPO`](Self::TAPS_TO_A_TEMPO) taps.
    ///
    /// Derived from the grid on every call, so it follows a player who tapped
    /// their way into a different tempo without anything being updated.
    pub fn tempo(&self) -> Option<f64> {
        if self.grid.len() < Self::TAPS_TO_A_TEMPO {
            return None;
        }

        self.grid.beats_per_minute()
    }

    fn stale_after(&self) -> u64 {
        Self::STALE_AFTER_SECONDS * u64::from(self.grid.sample_rate())
    }

    fn resembles_the_sequence(&self, interval: u64) -> bool {
        let Some(average) = self.average_interval() else {
            return true;
        };

        interval <= average * OUTLYING_RATIO && interval * OUTLYING_RATIO >= average
    }

    fn average_interval(&self) -> Option<u64> {
        let intervals = self.grid.len().checked_sub(1).filter(|&count| count > 0)?;
        let span = self.grid.beats().last()? - self.grid.beats().first()?;

        Some(span / intervals as u64)
    }

    fn restart(&mut self, at: u64) -> bool {
        self.grid = BeatGrid::new(self.grid.sample_rate());
        let _started = self.grid.push(at);

        false
    }
}

const OUTLYING_RATIO: u64 = 2;
