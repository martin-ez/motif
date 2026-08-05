//! Measuring how much of a block period the audio callback spent working, and
//! reading it elsewhere.
//!
//! Headroom rather than duration, because a duration says nothing on its own: a
//! callback taking 2 ms is comfortable at 256 frames and hopeless at 64.
//!
//! What crosses is the last block's fraction and the largest over a recent
//! window, packed into one word so a reader cannot catch half of each. The
//! running maximum is kept by the writer, so the shared word takes a plain
//! store rather than a compare-and-swap the callback cannot bound.
//!
//! Elapsed time arrives from the caller, so a test can state a block that took
//! half its period instead of spending one. The callback reads the clock with
//! [`Instant::now`](std::time::Instant::now), which reaches it without a syscall
//! through the Linux vDSO and the macOS commpage.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::device::DeviceProfile;

/// How much of the time a block was allowed to take the callback used.
///
/// Both are fractions of one block period, where 1.0 is a callback that used
/// exactly its deadline and anything above it is one that overran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Headroom {
    /// The most recent block's work as a fraction of that block's own period.
    pub load: f32,
    /// The largest [`load`](Self::load) over the recent window.
    ///
    /// This is the one that says whether the deadline is safe: a mean would hide
    /// the single late block, which is the whole of what a player hears.
    pub peak: f32,
}

impl Headroom {
    /// A callback that has done no work.
    pub const IDLE: Self = Self {
        load: 0.0,
        peak: 0.0,
    };

    /// The fraction of a block period the worst recent block left unused.
    ///
    /// Negative where that block overran rather than clamped at zero: how far
    /// past the deadline it went is the difference between work that is slightly
    /// too slow and work that is hopelessly too slow.
    ///
    /// ```
    /// use motif::audio::Headroom;
    ///
    /// assert_eq!(Headroom::IDLE.spare(), 1.0);
    /// ```
    pub fn spare(self) -> f32 {
        1.0 - self.peak
    }

    /// The tighter of two callbacks' readings.
    ///
    /// A duplex stream misses its deadline when either direction does, so the
    /// two are reported as one. Each number comes from whichever side was
    /// larger, which keeps a load from reading above the peak it arrives with:
    /// neither side's load exceeds its own peak, so neither exceeds the larger.
    ///
    /// ```
    /// use motif::audio::Headroom;
    ///
    /// let capture = Headroom { load: 0.6, peak: 0.7 };
    /// let render = Headroom { load: 0.1, peak: 0.9 };
    ///
    /// assert_eq!(capture.worse_of(render).peak, 0.9);
    /// ```
    pub fn worse_of(self, other: Self) -> Self {
        Self {
            load: self.load.max(other.load),
            peak: self.peak.max(other.peak),
        }
    }

    fn packed(self) -> u64 {
        u64::from(self.load.to_bits()) << 32 | u64::from(self.peak.to_bits())
    }

    fn unpacked(packed: u64) -> Self {
        Self {
            load: f32::from_bits((packed >> 32) as u32),
            peak: f32::from_bits(packed as u32),
        }
    }
}

/// Build a meter for a stream running at `sample_rate`, and split it into the
/// measuring end and the reading end.
///
/// Allocates here and never again, so this belongs in setup, before the stream
/// starts. A stream has one per callback rather than one in total, and the pair
/// reads back with [`Headroom::worse_of`].
///
/// The recent window spans two of [`DeviceProfile::TARGET`]'s screen frames: a
/// maximum has to outlast the gap between two readings or a spike falls between
/// them unseen, and the reader is a screen refreshing at that rate.
///
/// ```
/// use std::time::Duration;
///
/// let (mut writer, reader) = motif::audio::headroom_meter(48_000);
///
/// writer.measured(Duration::from_micros(500), 48);
///
/// assert_eq!(reader.read().load, 0.5);
/// ```
pub fn headroom_meter(sample_rate: u32) -> (HeadroomWriter, HeadroomReader) {
    let published = Arc::new(AtomicU64::new(Headroom::IDLE.packed()));

    (
        HeadroomWriter {
            published: Arc::clone(&published),
            sample_rate,
            window: Window::spanning(sample_rate),
        },
        HeadroomReader { published },
    )
}

/// The measuring end of a meter, held by whichever thread runs the callback.
pub struct HeadroomWriter {
    published: Arc<AtomicU64>,
    sample_rate: u32,
    window: Window,
}

impl HeadroomWriter {
    /// Publish a block that spent `elapsed` covering `frames` frames, and report
    /// what was published.
    ///
    /// The period comes from the frames the callback was handed rather than the
    /// block size the device was asked for: a host may hand over a short block,
    /// and a fraction measured against the wrong period is worse than none.
    ///
    /// A block of no frames, or a stream with no sample rate, has no period to
    /// be a fraction of. Neither is measured, and the previous reading stands.
    pub fn measured(&mut self, elapsed: Duration, frames: usize) -> Headroom {
        if frames == 0 || self.sample_rate == 0 {
            return Headroom::unpacked(self.published.load(Ordering::Relaxed));
        }

        let worked = elapsed.as_nanos() as f64 * f64::from(self.sample_rate);
        let period = frames as f64 * NANOSECONDS_PER_SECOND as f64;
        let load = (worked / period) as f32;

        let headroom = Headroom {
            load,
            peak: self.window.holding(load, frames as u64),
        };
        self.published.store(headroom.packed(), Ordering::Release);
        headroom
    }
}

/// The reading end of a meter, held by whichever thread reports it.
pub struct HeadroomReader {
    published: Arc<AtomicU64>,
}

impl HeadroomReader {
    /// The most recent block's reading, or [`Headroom::IDLE`] where no block has
    /// been measured yet.
    ///
    /// Reading takes nothing: a peak stays readable until the window it belongs
    /// to has passed, so two readers see the same spike and neither hides it
    /// from the other.
    ///
    /// The window advances with the blocks that are measured rather than with
    /// the clock, so a callback that has stopped being called keeps reporting
    /// the window it stopped in.
    pub fn read(&self) -> Headroom {
        Headroom::unpacked(self.published.load(Ordering::Acquire))
    }
}

const RECENT: Duration = DeviceProfile::TARGET
    .screen
    .frame_budget()
    .saturating_mul(2);

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

struct Window {
    spans: u64,
    covered: u64,
    holding: f32,
    held: f32,
}

impl Window {
    fn spanning(sample_rate: u32) -> Self {
        Self {
            spans: u64::from(sample_rate) * RECENT.as_nanos() as u64 / NANOSECONDS_PER_SECOND,
            covered: 0,
            holding: 0.0,
            held: 0.0,
        }
    }

    fn holding(&mut self, load: f32, frames: u64) -> f32 {
        self.holding = self.holding.max(load);
        self.covered += frames;

        let peak = self.holding.max(self.held);
        if self.covered >= self.spans {
            self.held = self.holding;
            self.holding = 0.0;
            self.covered = 0;
        }
        peak
    }
}
