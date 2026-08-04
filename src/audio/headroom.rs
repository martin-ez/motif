//! Measuring how much of a block period the audio callback spent working, and
//! reading it elsewhere.
//!
//! Headroom rather than duration, because a duration says nothing on its own: a
//! callback taking 2 ms is comfortable at 256 frames and hopeless at 64, and the
//! same DSP has a different deadline again on the target board. A fraction of
//! the period the block was allowed carries between machines, which is what
//! makes it the number worth recording.
//!
//! What crosses the boundary is the last block's fraction and the largest over a
//! recent window, packed into one word for the reason
//! [`level_meter`](super::level_meter) packs its pair: read as two atomics they
//! can straddle two blocks and come back as a pair no block ever had, a bar
//! drawn past its own peak-hold mark. The running maximum is kept in the
//! measuring end's own state rather than in the shared word, so the word has one
//! writer and a plain store — never a compare-and-swap, whose retry loop the
//! callback has no bound on.
//!
//! The elapsed time arrives from the caller rather than being read here, so that
//! a test can state a block that took half its period instead of spending one.
//! The callback reads it with [`Instant::now`](std::time::Instant::now), which
//! allocates on no platform and reaches the clock without trapping into the
//! kernel: on Linux it resolves through the vDSO, and on macOS through the
//! commpage.

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
    /// This is the one that says whether the deadline is safe: a mean hides the
    /// single late block a player hears, and one late block is the whole of what
    /// went wrong.
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
    /// Negative where that block overran, which is a real reading rather than a
    /// floor to clamp at: how far past the deadline it went is what says whether
    /// the work is slightly or hopelessly too slow.
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
/// starts. A stream has one per callback rather than one in total: two threads
/// sharing a meter would need the retry loop keeping them apart out of the
/// callback, and reading the pair back with [`Headroom::worse_of`] costs
/// nothing.
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
    /// The period is taken from the frames the callback was actually handed
    /// rather than from the block size the device was asked for, because a host
    /// is free to hand over a short block and a fraction measured against the
    /// wrong period is worse than none.
    ///
    /// A block of no frames, and a meter for a stream with no sample rate, have
    /// no period to be a fraction of. Neither is measured, and the previous
    /// reading stands rather than being replaced by a zero that would say the
    /// callback had been idle.
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
    pub fn read(&self) -> Headroom {
        Headroom::unpacked(self.published.load(Ordering::Acquire))
    }
}

/// The span a recent maximum covers, as the frames the target draws one frame
/// in.
///
/// A maximum has to outlast the gap between two readings or a spike falls
/// between them unseen, and the reader is a screen refreshing at
/// [`DeviceProfile::TARGET`]'s rate.
const RECENT: Duration = DeviceProfile::TARGET.screen.frame_budget();

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
