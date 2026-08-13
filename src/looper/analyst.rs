//! The pass that reads a finished take, off the thread that recorded it.
//!
//! Nothing here is new analysis. [`Envelope`](crate::analysis::Envelope) and
//! [`track`] already answer over a whole take; what this adds is that they run
//! in that order, on a worker rather than on the callback, over the samples a
//! player closed a loop with — and that what the player said about the take
//! reaches the tracker as [`Priors`] rather than being inferred again.
//!
//! The worker looks rather than waits, because the end publishing takes is the
//! audio callback and a callback may not wake a thread.

use std::thread;
use std::time::Duration;

use crate::analysis::{Priors, track};

use super::{FinishedTake, LoopMarks, Mark, MarksWriter, TakeReader};

const LOOK_EVERY: Duration = Duration::from_millis(10);
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Analyse every take `takes` hands over, publishing what each one held to
/// `found`.
///
/// Spawns the worker and returns, so the caller is free to be a callback or a
/// frame. The worker owns the reading end of the handoff for the rest of the
/// run: there is one loop and one analyst, both built in setup, and nothing it
/// holds needs releasing before the process ends.
///
/// Samples are timed against `sample_rate`, which is the rate the loop was
/// captured at.
pub fn analysing(takes: TakeReader, sample_rate: u32, found: MarksWriter) {
    let mut takes = takes;
    let mut found = found;

    thread::spawn(move || {
        loop {
            if let Some(take) = takes.claim() {
                found.publish(analyse(&take, sample_rate));
            }
            thread::sleep(LOOK_EVERY);
        }
    });
}

/// What `take` holds, as the marks a page draws over it.
///
/// The length the player closed the loop at and the count they stated are the
/// [`Priors`] the grid is placed under; a take nobody counted is placed under
/// its length alone. Beats are timestamps, so they come back as the frames of
/// the take they fall at, timed against `sample_rate`.
///
/// ```
/// use motif::device::DeviceProfile;
/// use motif::looper::{LoopBuffer, LoopMarks, TakeWriter, analyse, take_handoff};
///
/// let profile = DeviceProfile::TARGET.audio;
/// let (mut writer, mut reader) = take_handoff(profile);
/// let mut buffer = LoopBuffer::for_profile(profile);
/// buffer.record(&[0.0; 4]);
///
/// writer.begin(&buffer, None);
/// for _ in 0..TakeWriter::CROSSING_BLOCKS {
///     writer.advance(&buffer, profile.block_size as usize);
/// }
///
/// let take = reader.claim().expect("a finished take crossed");
/// assert_eq!(analyse(&take, profile.sample_rate), LoopMarks::none());
/// ```
pub fn analyse(take: &FinishedTake<'_>, sample_rate: u32) -> LoopMarks {
    let tracked = track(take.samples(), sample_rate, priors_of(take, sample_rate));
    let mut marks = LoopMarks::none();

    for beat in tracked.beats() {
        marks.add(frame_of(*beat, sample_rate), Mark::Beat);
    }
    for downbeat in tracked.downbeats() {
        marks.add(frame_of(downbeat, sample_rate), Mark::Downbeat);
    }

    marks
}

fn priors_of(take: &FinishedTake<'_>, sample_rate: u32) -> Priors {
    let priors = Priors::of_take(span_of(take.frames(), sample_rate));

    match take.bars() {
        Some(bars) => priors.with_meter(bars.beats_each()).with_bars(bars.count()),
        None => priors,
    }
}

fn span_of(frames: usize, sample_rate: u32) -> Duration {
    Duration::from_nanos((frames as u128 * NANOS_PER_SECOND / u128::from(sample_rate)) as u64)
}

fn frame_of(when: Duration, sample_rate: u32) -> u64 {
    (when.as_nanos() * u128::from(sample_rate) / NANOS_PER_SECOND) as u64
}
