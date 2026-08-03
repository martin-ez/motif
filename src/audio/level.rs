//! Publishing how loud a block was, from the audio callback to the application
//! thread.
//!
//! A meter is not a queue. The thread drawing it wants the level now, not every
//! level since it last looked, so a block published while nobody was reading is
//! overwritten rather than kept. That makes the whole crossing one atomic store
//! against one atomic load: wait-free at both ends, and a fixed cost per block
//! on the end that may not wait.
//!
//! Peak and RMS travel packed into a single [`AtomicU64`] rather than in two
//! atomics side by side. Read separately they can straddle two blocks, and the
//! pair that comes back is then one no block ever had — an RMS above the peak
//! it arrives with, which a meter would draw as a bar past its own clip mark.
//! Packed, there is nothing to straddle.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// How loud a block of samples was.
///
/// Both are linear amplitudes on the scale the samples themselves use, where
/// 1.0 is full scale. A meter drawn in decibels converts on its own side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    /// The largest absolute sample in the block.
    ///
    /// This is what catches clipping: one sample at full scale shows here and
    /// is all but invisible in [`rms`](Self::rms).
    pub peak: f32,
    /// The root mean square of the block.
    ///
    /// This is what tracks loudness as a player hears it, being an average over
    /// the block rather than its worst instant. A sine reads about 3 dB below
    /// its own peak, which is why a meter showing only one of the two misleads
    /// in one direction or the other.
    pub rms: f32,
}

impl Levels {
    /// A block with no signal in it.
    pub const SILENT: Self = Self {
        peak: 0.0,
        rms: 0.0,
    };

    /// Measure `samples`.
    ///
    /// Channel layout does not matter: interleaved samples are measured across
    /// every channel at once, which is what a meter watching for clipping
    /// wants. Folding the channels together first would let one channel at full
    /// scale hide against another.
    ///
    /// A block with no samples in it measures as [`SILENT`](Self::SILENT), and
    /// so does a sample that is not finite. A driver is free to hand back
    /// whatever is in its buffer, and left alone one such sample poisons the
    /// whole block: comparison ignores a NaN where addition carries it, so the
    /// block would read as an infinite peak beside an RMS of NaN — a pair no
    /// block ever had, which is the thing packing the two into one word exists
    /// to prevent.
    ///
    /// ```
    /// use motif::audio::Levels;
    ///
    /// let levels = Levels::of(&[0.2, -0.8, 0.4, 0.0]);
    ///
    /// assert_eq!(levels.peak, 0.8);
    /// assert!(levels.rms < levels.peak);
    /// ```
    pub fn of(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::SILENT;
        }

        let mut peak = 0.0f32;
        let mut squares = 0.0f32;
        for sample in samples {
            let magnitude = sample.abs();
            if magnitude.is_finite() {
                peak = peak.max(magnitude);
                squares += magnitude * magnitude;
            }
        }

        Self {
            peak,
            rms: (squares / samples.len() as f32).sqrt(),
        }
    }

    fn packed(self) -> u64 {
        (u64::from(self.peak.to_bits()) << 32) + u64::from(self.rms.to_bits())
    }

    fn unpacked(packed: u64) -> Self {
        Self {
            peak: f32::from_bits((packed >> 32) as u32),
            rms: f32::from_bits(packed as u32),
        }
    }
}

/// Build a meter, and split it into the end that publishes and the end that
/// reads.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// let (mut writer, reader) = motif::audio::level_meter();
///
/// writer.publish(&[0.5, -0.5]);
///
/// assert_eq!(reader.read().peak, 0.5);
/// ```
pub fn level_meter() -> (LevelWriter, LevelReader) {
    let published = Arc::new(AtomicU64::new(Levels::SILENT.packed()));

    (
        LevelWriter {
            published: Arc::clone(&published),
        },
        LevelReader { published },
    )
}

/// The measuring end of a meter, held by whichever thread produces samples.
///
/// This is the end the audio callback holds.
pub struct LevelWriter {
    published: Arc<AtomicU64>,
}

impl LevelWriter {
    /// Measure `samples`, publish the result, and report what was published.
    ///
    /// Whatever was published before is replaced rather than queued: a block
    /// the reader never looked at is gone, which is the whole of what makes
    /// this safe to call from a callback that cannot wait for a reader.
    pub fn publish(&mut self, samples: &[f32]) -> Levels {
        let levels = Levels::of(samples);
        self.published.store(levels.packed(), Ordering::Release);
        levels
    }
}

/// The reading end of a meter, held by whichever thread displays it.
///
/// This is the end the application thread holds.
pub struct LevelReader {
    published: Arc<AtomicU64>,
}

impl LevelReader {
    /// The most recently published block, or [`Levels::SILENT`] where no block
    /// has been published yet.
    ///
    /// Reading takes nothing: the same block reads the same way until the next
    /// one replaces it, so a display running faster than the audio callback
    /// repeats a value rather than finding nothing there.
    pub fn read(&self) -> Levels {
        Levels::unpacked(self.published.load(Ordering::Acquire))
    }
}
