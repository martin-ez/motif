//! The shape of the loop, published from the thread that owns it to the thread
//! that draws it.
//!
//! Samples themselves cannot cross. The loop is megabytes and lives under
//! invariant 2, so neither a lock nor a copy per drawn frame is available. What
//! crosses is a fixed number of peak and trough pairs, folded in as the loop is
//! recorded, costing the callback one pass over a block it already has.
//!
//! Buckets are fixed in count and not in width, because a loop's length is not
//! known until recording stops. A bucket starts at one frame and widens to the
//! next power of two that covers the loop, folding the buckets it swallows
//! together: the summary spans the whole loop at every length, and it does so
//! in one bounded pass over an array rather than a loop that runs until it
//! fits.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering, fence};

const BUCKET_COUNT: usize = 128;
const FULL_SCALE: f32 = 2.0;
const LEVELS_PER_CELL: usize = 8;
const BLANK: char = ' ';
const BLOCKS: [char; LEVELS_PER_CELL] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const BITS_PER_SAMPLE: u32 = 32;

/// The largest and smallest sample in a span of the loop.
///
/// Linear amplitudes on the scale the samples use, where 1.0 is full scale. The
/// pair rather than a single magnitude, because a span is drawn as the swing
/// between them: a signal sitting above zero is a different shape from one
/// swinging through it, at the same loudness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extremes {
    /// The largest sample in the span.
    pub peak: f32,
    /// The smallest sample in the span.
    pub trough: f32,
}

impl Extremes {
    /// A span with no signal in it, which is also how a bucket no frame has
    /// reached reads.
    pub const SILENT: Self = Self {
        peak: 0.0,
        trough: 0.0,
    };

    fn including(self, sample: f32) -> Self {
        Self {
            peak: self.peak.max(sample),
            trough: self.trough.min(sample),
        }
    }

    fn merged(self, other: Self) -> Self {
        Self {
            peak: self.peak.max(other.peak),
            trough: self.trough.min(other.trough),
        }
    }

    fn between(self, other: Self, part: f32) -> Self {
        Self {
            peak: self.peak + (other.peak - self.peak) * part,
            trough: self.trough + (other.trough - self.trough) * part,
        }
    }

    fn span(self) -> f32 {
        self.peak - self.trough
    }

    fn packed(self) -> u64 {
        u64::from(self.peak.to_bits()) << BITS_PER_SAMPLE | u64::from(self.trough.to_bits())
    }

    fn unpacked(packed: u64) -> Self {
        Self {
            peak: f32::from_bits((packed >> BITS_PER_SAMPLE) as u32),
            trough: f32::from_bits(packed as u32),
        }
    }
}

/// The loop summarised as peak and trough pairs across its length.
///
/// ```
/// use motif::looper::LoopWaveform;
///
/// let mut waveform = LoopWaveform::EMPTY;
/// waveform.take(0, [0.5, -0.5, 0.0, 0.0]);
///
/// assert_eq!(waveform.buckets().len(), 4);
/// assert_eq!(waveform.drawn(2, 1), ["▄ "]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopWaveform {
    buckets: [Extremes; BUCKET_COUNT],
    width: usize,
    length: usize,
}

impl LoopWaveform {
    /// How many peak and trough pairs the summary holds.
    ///
    /// Twice the columns of
    /// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET)'s screen,
    /// so a region is the narrower side of the mapping for all but the shortest
    /// loops and the drawing keeps extremes rather than inventing them between
    /// them. A constant rather than a setting, because the callback that fills
    /// it may not allocate.
    pub const BUCKETS: usize = BUCKET_COUNT;

    /// A summary of no loop at all.
    pub const EMPTY: Self = Self {
        buckets: [Extremes::SILENT; BUCKET_COUNT],
        width: 1,
        length: 0,
    };

    /// Fold `samples`, which begin at frame `from` of the loop, into the
    /// summary.
    ///
    /// A bucket entered at its first frame is replaced rather than merged into,
    /// so a layer sweeping the loop a second time repaints the buckets it
    /// passes instead of piling onto them. The buckets are widened first, in a
    /// single pass over the array, which is what keeps them spanning the whole
    /// loop however far this call carries its end.
    pub fn take<S>(&mut self, from: usize, samples: S)
    where
        S: IntoIterator<Item = f32>,
        S::IntoIter: ExactSizeIterator,
    {
        let samples = samples.into_iter();
        self.length = self.length.max(from + samples.len());
        self.spread_over(self.length.div_ceil(BUCKET_COUNT).next_power_of_two());

        for (offset, sample) in samples.enumerate() {
            self.folding(from + offset, sample);
        }
    }

    /// The buckets that carry the loop, from its first frame to its last.
    ///
    /// Only the ones a frame has reached: no loop has none, and a loop shorter
    /// than [`BUCKETS`](Self::BUCKETS) frames has one a frame.
    pub fn buckets(&self) -> &[Extremes] {
        &self.buckets[..self.length.div_ceil(self.width)]
    }

    /// The loop drawn `rows` rows tall and `columns` columns wide, top row
    /// first.
    ///
    /// Bars grow from the bottom, their height the peak-to-trough swing against
    /// full scale, and eight block glyphs give a cell eight levels. A column
    /// covering several buckets takes their extreme rather than their mean,
    /// because a peak that averages away is the one a player is looking for; a
    /// loop of fewer buckets than columns is interpolated between them.
    pub fn drawn(&self, columns: usize, rows: usize) -> Vec<String> {
        (0..rows)
            .map(|row| {
                (0..columns)
                    .map(|column| glyph(self.column(column, columns), row, rows))
                    .collect()
            })
            .collect()
    }

    fn folding(&mut self, frame: usize, sample: f32) {
        let bucket = frame / self.width;
        if frame.is_multiple_of(self.width) {
            self.buckets[bucket] = Extremes::SILENT;
        }
        self.buckets[bucket] = self.buckets[bucket].including(sample);
    }

    fn spread_over(&mut self, width: usize) {
        let factor = width / self.width;
        let kept = BUCKET_COUNT / factor;

        for bucket in 0..kept {
            let from = bucket * factor;
            self.buckets[bucket] = self.buckets[from..from + factor]
                .iter()
                .fold(Extremes::SILENT, |merged, extremes| {
                    merged.merged(*extremes)
                });
        }
        self.buckets[kept..].fill(Extremes::SILENT);
        self.width = width;
    }

    fn column(&self, at: usize, columns: usize) -> Extremes {
        let buckets = self.buckets();
        let Some(last) = buckets.len().checked_sub(1) else {
            return Extremes::SILENT;
        };

        if buckets.len() >= columns {
            return buckets[covered(at, buckets.len(), columns)]
                .iter()
                .fold(Extremes::SILENT, |merged, bucket| merged.merged(*bucket));
        }

        let part = (at * last) as f32 / (columns - 1) as f32;
        let first = part as usize;
        match buckets.get(first + 1) {
            Some(next) => buckets[first].between(*next, part - first as f32),
            None => buckets[first],
        }
    }
}

fn covered(at: usize, buckets: usize, columns: usize) -> std::ops::Range<usize> {
    at * buckets / columns..(at + 1) * buckets / columns
}

fn glyph(extremes: Extremes, row: usize, rows: usize) -> char {
    let levels = rows * LEVELS_PER_CELL;
    let lit = (extremes.span() / FULL_SCALE * levels as f32).clamp(0.0, levels as f32) as usize;
    let level = lit
        .saturating_sub((rows - 1 - row) * LEVELS_PER_CELL)
        .min(LEVELS_PER_CELL);

    match level.checked_sub(1) {
        Some(block) => BLOCKS[block],
        None => BLANK,
    }
}

/// Build a waveform meter, and split it into the end that publishes and the end
/// that reads.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// use motif::looper::{LoopWaveform, waveform_meter};
///
/// let (mut writer, reader) = waveform_meter();
/// let mut waveform = LoopWaveform::EMPTY;
/// waveform.take(0, [0.5, -0.5]);
///
/// writer.publish(&waveform);
///
/// assert_eq!(reader.read(), waveform);
/// ```
pub fn waveform_meter() -> (WaveformWriter, WaveformReader) {
    let published = Arc::new(Published {
        sequence: AtomicU32::new(0),
        buckets: [const { AtomicU64::new(0) }; BUCKET_COUNT],
        width: AtomicUsize::new(LoopWaveform::EMPTY.width),
        length: AtomicUsize::new(LoopWaveform::EMPTY.length),
    });

    (
        WaveformWriter {
            published: Arc::clone(&published),
        },
        WaveformReader { published },
    )
}

struct Published {
    sequence: AtomicU32,
    buckets: [AtomicU64; BUCKET_COUNT],
    width: AtomicUsize,
    length: AtomicUsize,
}

/// The publishing end of a waveform meter, held by whichever thread owns the
/// loop.
///
/// This is the end the audio callback holds.
pub struct WaveformWriter {
    published: Arc<Published>,
}

impl WaveformWriter {
    /// Publish `waveform`, replacing whatever was there.
    ///
    /// A fixed number of stores into storage that is already there, waiting on
    /// nobody: a summary the reader never looked at is gone, which is what
    /// makes this safe on a callback. The count either side of the stores is
    /// what makes the summary arrive whole, a reader catching it mid-write
    /// reading again rather than drawing halves of two loops.
    pub fn publish(&mut self, waveform: &LoopWaveform) {
        let writing = self
            .published
            .sequence
            .load(Ordering::Relaxed)
            .wrapping_add(1);
        self.published.sequence.store(writing, Ordering::Relaxed);
        fence(Ordering::Release);

        for (cell, bucket) in self.published.buckets.iter().zip(waveform.buckets) {
            cell.store(bucket.packed(), Ordering::Relaxed);
        }
        self.published
            .width
            .store(waveform.width, Ordering::Relaxed);
        self.published
            .length
            .store(waveform.length, Ordering::Relaxed);

        self.published
            .sequence
            .store(writing.wrapping_add(1), Ordering::Release);
    }
}

/// The reading end of a waveform meter, held by whichever thread draws it.
///
/// This is the end the application thread holds.
pub struct WaveformReader {
    published: Arc<Published>,
}

impl WaveformReader {
    /// The most recently published summary, or [`LoopWaveform::EMPTY`] where
    /// none has been published yet.
    ///
    /// Reading takes nothing, so a screen running faster than the callback
    /// repeats a summary rather than finding nothing there. A read caught
    /// against a publish is taken again, which puts the waiting on the thread
    /// that draws and never on the one that may not wait.
    pub fn read(&self) -> LoopWaveform {
        loop {
            let before = self.published.sequence.load(Ordering::Acquire);
            if !before.is_multiple_of(2) {
                continue;
            }

            let mut waveform = LoopWaveform::EMPTY;
            for (bucket, cell) in waveform.buckets.iter_mut().zip(&self.published.buckets) {
                *bucket = Extremes::unpacked(cell.load(Ordering::Relaxed));
            }
            waveform.width = self.published.width.load(Ordering::Relaxed);
            waveform.length = self.published.length.load(Ordering::Relaxed);
            fence(Ordering::Acquire);

            if self.published.sequence.load(Ordering::Relaxed) == before {
                return waveform;
            }
        }
    }
}
