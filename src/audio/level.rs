//! Publishing how loud a block was, from the audio callback to the application
//! thread.
//!
//! A meter is not a queue. The thread drawing it wants the level now, not every
//! level since it last looked, so a block published while nobody was reading is
//! overwritten. The whole crossing is then one atomic store against one atomic
//! load: wait-free at both ends, and a fixed cost on the end that may not wait.
//!
//! Peak and RMS travel packed into a single [`AtomicU64`] rather than in two
//! atomics side by side: read separately they can straddle two blocks, and the
//! pair that comes back is one no block ever had — an RMS above its own peak.
//!
//! A meter is built for one stream's channel layout and counts only the
//! channels that stream captures. Metering the rest reports a level on audio
//! nobody is recording: a hot line on an unselected input would read as clipping.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::ChannelSelection;

/// How loud a block of samples was.
///
/// Both are linear amplitudes on the scale the samples themselves use, where
/// 1.0 is full scale, and a meter drawn in decibels converts on its own side. A
/// sample that is not finite is left out of both: a driver may hand back
/// whatever was in its buffer, and comparison ignores a NaN where addition
/// carries it.
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
    /// A block with no signal in it, and what a block holding no whole frame
    /// measures as.
    pub const SILENT: Self = Self {
        peak: 0.0,
        rms: 0.0,
    };

    fn measure(samples: &[f32], channels: usize, selected: Range<usize>) -> Self {
        let mut measured = Measured::default();
        for frame in samples.chunks_exact(channels) {
            measured.take(&frame[selected.clone()]);
        }
        measured.levels()
    }

    fn packed(self) -> u64 {
        u64::from(self.peak.to_bits()) << 32 | u64::from(self.rms.to_bits())
    }

    fn unpacked(packed: u64) -> Self {
        Self {
            peak: f32::from_bits((packed >> 32) as u32),
            rms: f32::from_bits(packed as u32),
        }
    }
}

#[derive(Default)]
struct Measured {
    peak: f32,
    squares: f32,
    counted: usize,
}

impl Measured {
    fn take(&mut self, samples: &[f32]) {
        for sample in samples {
            let magnitude = sample.abs();
            if magnitude.is_finite() {
                self.peak = self.peak.max(magnitude);
                self.squares += magnitude * magnitude;
            }
        }
        self.counted += samples.len();
    }

    fn levels(&self) -> Levels {
        if self.counted == 0 {
            return Levels::SILENT;
        }
        Levels {
            peak: self.peak,
            rms: (self.squares / self.counted as f32).sqrt(),
        }
    }
}

/// Build a meter over `selection` of a block `channels` wide, and split it into
/// the end that publishes and the end that reads.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts. A selection reaching past `channels` is narrowed
/// to what the block holds, which is what keeps the publishing end free of a
/// path that can panic.
///
/// ```
/// use motif::audio::{ChannelSelection, level_meter};
///
/// let (mut writer, reader) = level_meter(1, ChannelSelection::all(1));
///
/// writer.publish(&[0.5, -0.5]);
///
/// assert_eq!(reader.read().peak, 0.5);
/// ```
///
/// The second channel of a stereo block is a loud line nobody selected, and the
/// meter does not see it:
///
/// ```
/// use motif::audio::{ChannelSelection, level_meter};
///
/// let (mut writer, reader) = level_meter(2, ChannelSelection { first: 0, count: 1 });
///
/// writer.publish(&[0.25, 1.0, 0.25, 1.0]);
///
/// assert_eq!(reader.read().peak, 0.25);
/// ```
pub fn level_meter(channels: u16, selection: ChannelSelection) -> (LevelWriter, LevelReader) {
    let published = Arc::new(AtomicU64::new(Levels::SILENT.packed()));

    (
        LevelWriter {
            published: Arc::clone(&published),
            channels: channels as usize,
            selected: reachable(channels, selection),
        },
        LevelReader { published },
    )
}

fn reachable(channels: u16, selection: ChannelSelection) -> Range<usize> {
    let width = channels as u32;
    selection.first.min(channels) as usize..selection.reach().min(width) as usize
}

/// The measuring end of a meter, held by whichever thread produces samples.
///
/// This is the end the audio callback holds.
pub struct LevelWriter {
    published: Arc<AtomicU64>,
    channels: usize,
    selected: Range<usize>,
}

impl LevelWriter {
    /// Measure the selected channels of the interleaved block `samples`,
    /// publish the result, and report what was published.
    ///
    /// Whatever was published before is replaced rather than queued: a block
    /// the reader never looked at is gone, which is the whole of what makes
    /// this safe to call from a callback that cannot wait for a reader.
    ///
    /// `samples` may be any length; anything past the last whole frame is
    /// ignored, as it is on the way into the boundary.
    pub fn publish(&mut self, samples: &[f32]) -> Levels {
        let levels = Levels::measure(samples, self.channels, self.selected.clone());
        self.published.store(levels.packed(), Ordering::Release);
        levels
    }

    /// Publish that there was nothing to measure.
    ///
    /// A producer with no block to hand over still has a level, and it is
    /// [`Levels::SILENT`] rather than whatever it published last: a meter left
    /// showing the block before is a meter that reports a signal nobody is
    /// making.
    pub fn silence(&mut self) {
        self.published
            .store(Levels::SILENT.packed(), Ordering::Release);
    }
}

/// The reading end of a meter, held by whichever thread displays it.
///
/// This is the end the application thread holds. It clones, so a meter built
/// inside something that is on its way to a callback can still be read from:
/// what a clone shares is the one reading, not a second meter.
#[derive(Clone)]
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
