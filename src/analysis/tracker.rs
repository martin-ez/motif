//! Placing a grid of beats over a take, and finding which of them begin a bar.
//!
//! A pulse is chosen by how much of the take's onset strength the grid it
//! implies explains, and the grid itself is the best path through that
//! envelope at the pulse — best over the whole take rather than beat by beat,
//! which is what stops one onset played early from taking the beats after it
//! with it.

use std::time::Duration;

use super::Envelope;

/// The slowest pulse a grid is placed at, in beats per minute.
///
/// A hand-played loop falls inside 60 to 200 BPM, and the bounds are what stop
/// a grid explaining the audio at a rate nobody played.
pub const SLOWEST: f64 = 60.0;

/// The fastest pulse a grid is placed at, in beats per minute.
///
/// The upper bound of the range [`SLOWEST`] opens.
pub const FASTEST: f64 = 200.0;

const PREFERRED: f64 = 120.0;
const SPREAD: f64 = 0.9;
const TIGHTNESS: f64 = 10.0;
const WANDER: f64 = 0.20;
const SWEEP_STEP: f64 = 1.03;
const SECONDS_PER_MINUTE: f64 = 60.0;

/// What a manual looper knows about a take that a beat tracker does not.
///
/// The player closed the loop, so its length is a fact rather than an
/// estimate, and a pulse has to divide it into a whole number of beats. How
/// the take divides into bars is the other half, and nothing in the engine
/// carries it yet, so both halves of that are optional and
/// [`ASSUMED_BAR`](Self::ASSUMED_BAR) stands in for the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priors {
    length: Option<Duration>,
    beats_per_bar: Option<usize>,
    bars: Option<usize>,
}

impl Priors {
    /// How many beats go to a bar where nothing says.
    ///
    /// Four, which covers most of what anyone loops and fails loudly rather
    /// than quietly: a waltz tracked against it puts a downbeat on the wrong
    /// beat of every bar rather than slightly out of place.
    pub const ASSUMED_BAR: usize = 4;

    /// Knowing nothing about the take, as a general beat tracker has it.
    pub fn blind() -> Self {
        Self {
            length: None,
            beats_per_bar: None,
            bars: None,
        }
    }

    /// Knowing that the take is `length` long and repeats there.
    pub fn of_take(length: Duration) -> Self {
        Self {
            length: Some(length),
            ..Self::blind()
        }
    }

    /// Also knowing that a bar is `beats_per_bar` beats.
    ///
    /// A bar of no beats is not a bar, so it is refused and
    /// [`ASSUMED_BAR`](Self::ASSUMED_BAR) is used instead.
    pub fn with_meter(self, beats_per_bar: usize) -> Self {
        Self {
            beats_per_bar: (beats_per_bar > 0).then_some(beats_per_bar),
            ..self
        }
    }

    /// Also knowing that the take runs `bars` bars.
    ///
    /// Together with a meter this is the whole of the pulse but its phase: the
    /// take holds one known number of beats, so nothing is left to choose
    /// between a pulse and its own double, and the grid is laid holding
    /// exactly that many beats rather than as many as the take has room for.
    /// A take of no bars is refused, as a bar of no beats is.
    pub fn with_bars(self, bars: usize) -> Self {
        Self {
            bars: (bars > 0).then_some(bars),
            ..self
        }
    }

    fn bar(&self) -> usize {
        self.beats_per_bar.unwrap_or(Self::ASSUMED_BAR)
    }

    fn counted(&self) -> Option<usize> {
        self.bars.map(|bars| bars * self.bar())
    }

    fn whole_bars(&self, beats: u32) -> bool {
        self.beats_per_bar
            .is_none_or(|per_bar| (beats as usize).is_multiple_of(per_bar))
    }
}

/// The beats found in a take, and where its bars begin.
///
/// The beats are timestamps from the start of the take and nothing here is a
/// tempo: what the take was played at is arithmetic over these, and keeping a
/// number beside them would discard the very drift they were placed to follow.
#[derive(Debug, Clone, PartialEq)]
pub struct Tracked {
    beats: Vec<Duration>,
    beats_per_bar: usize,
    downbeat: usize,
}

impl Tracked {
    /// Every beat, in the order they fall.
    pub fn beats(&self) -> &[Duration] {
        &self.beats
    }

    /// The beats that begin a bar, which are every
    /// [`beats_per_bar`](Self::beats_per_bar)-th of them.
    pub fn downbeats(&self) -> impl Iterator<Item = Duration> + '_ {
        self.beats
            .iter()
            .skip(self.downbeat)
            .step_by(self.beats_per_bar)
            .copied()
    }

    /// How many beats the bars were counted in.
    pub fn beats_per_bar(&self) -> usize {
        self.beats_per_bar
    }
}

/// Find the beats of a take, and which of them begin a bar.
///
/// The grid is the path through the onset envelope that best trades landing
/// on what was played against holding the interval it was placed at, as
/// Ellis's beat tracker weighs it: the squared log of the ratio an interval
/// bears to the pulse, at a tightness of ten — his six, stiffened against the
/// development fixtures, where six let a grid slip onto a syncopated
/// subdivision and back. The pulse is chosen under a log-Gaussian preference
/// for 120 BPM nine tenths of an octave wide, or nothing prefers its double.
///
/// ```
/// use motif::analysis::{Priors, track};
/// use std::time::Duration;
///
/// let struck = |frame: usize| if frame % 4_000 < 100 { 0.5 } else { 0.0 };
/// let take = Duration::from_secs(4);
/// let found = track((0..32_000).map(struck), 8_000, Priors::of_take(take));
///
/// assert_eq!(found.beats().len(), 8);
/// ```
pub fn track(samples: impl IntoIterator<Item = f32>, sample_rate: u32, priors: Priors) -> Tracked {
    let envelope = Envelope::of(samples, sample_rate);
    let beats = strongest_grid(&envelope, priors).unwrap_or_default();
    let downbeat = bar_phase(&envelope, &beats, priors.bar());

    Tracked {
        beats,
        beats_per_bar: priors.bar(),
        downbeat,
    }
}

fn strongest_grid(envelope: &Envelope, priors: Priors) -> Option<Vec<Duration>> {
    let onsets = normalised(envelope)?;
    let hop = envelope.hop();
    let mut strongest: Option<(f64, Vec<Duration>)> = None;

    for period in candidates(priors) {
        let over = frames_in(period, hop);
        let (frames, explained) = match priors.counted() {
            Some(count) => counted_path(&onsets, over, count),
            None => best_path(&onsets, over),
        };
        let score = explained * preference(period);
        if score > strongest.as_ref().map_or(0.0, |(best, _)| *best) {
            let beats = frames.into_iter().map(|frame| hop * frame as u32).collect();
            strongest = Some((score, beats));
        }
    }

    strongest.map(|(_, beats)| beats)
}

fn normalised(envelope: &Envelope) -> Option<Vec<f64>> {
    let strength = envelope.strength();
    let strongest = strength
        .iter()
        .fold(0.0_f32, |strongest, risen| strongest.max(*risen));

    (strongest > 0.0).then(|| {
        strength
            .iter()
            .map(|risen| f64::from(*risen / strongest))
            .collect()
    })
}

fn best_path(onsets: &[f64], period: usize) -> (Vec<usize>, f64) {
    let shortest = period.saturating_sub(wander(period)).max(1);
    let longest = period + wander(period);
    let mut score = vec![0.0; onsets.len()];
    let mut before: Vec<Option<usize>> = vec![None; onsets.len()];

    for frame in 0..onsets.len() {
        let mut best = 0.0;
        for interval in shortest..=longest.min(frame) {
            let kept = score[frame - interval] - TIGHTNESS * regularity(interval, period);
            if kept > best {
                best = kept;
                before[frame] = Some(frame - interval);
            }
        }
        score[frame] = best + onsets[frame];
    }

    walked_back(onsets, &score, &before)
}

fn counted_path(onsets: &[f64], period: usize, count: usize) -> (Vec<usize>, f64) {
    let shortest = period.saturating_sub(wander(period)).max(1);
    let longest = period + wander(period);
    let mut score = vec![vec![f64::NEG_INFINITY; onsets.len()]; count + 1];
    let mut before: Vec<Vec<Option<usize>>> = vec![vec![None; onsets.len()]; count + 1];

    score[1].copy_from_slice(onsets);

    for placed in 2..=count {
        for frame in 0..onsets.len() {
            let mut best = f64::NEG_INFINITY;
            for interval in shortest..=longest.min(frame) {
                let kept =
                    score[placed - 1][frame - interval] - TIGHTNESS * regularity(interval, period);
                if kept > best {
                    best = kept;
                    before[placed][frame] = Some(frame - interval);
                }
            }
            score[placed][frame] = best + onsets[frame];
        }
    }

    walked_back_counting(onsets, &score, &before, count)
}

fn walked_back_counting(
    onsets: &[f64],
    score: &[Vec<f64>],
    before: &[Vec<Option<usize>>],
    count: usize,
) -> (Vec<usize>, f64) {
    let last = &score[count];
    let mut frame = (0..onsets.len()).max_by(|one, other| last[*one].total_cmp(&last[*other]));
    let mut placed = count;
    let mut beats = Vec::new();
    let mut explained = 0.0;

    while let Some(at) = frame {
        beats.push(at);
        explained += onsets[at];
        frame = before[placed][at];
        placed -= 1;
    }
    beats.reverse();

    (beats, explained)
}

fn wander(period: usize) -> usize {
    ((period as f64 * WANDER).round() as usize).max(1)
}

fn regularity(interval: usize, period: usize) -> f64 {
    (interval as f64 / period as f64).ln().powi(2)
}

fn walked_back(onsets: &[f64], score: &[f64], before: &[Option<usize>]) -> (Vec<usize>, f64) {
    let mut frame = (0..score.len()).max_by(|one, other| score[*one].total_cmp(&score[*other]));
    let mut beats = Vec::new();
    let mut explained = 0.0;

    while let Some(at) = frame {
        beats.push(at);
        explained += onsets[at];
        frame = before[at];
    }
    beats.reverse();

    (beats, explained)
}

fn candidates(priors: Priors) -> Vec<Duration> {
    match (priors.length, priors.counted()) {
        (Some(length), Some(beats)) => vec![length / beats as u32],
        (Some(length), None) => dividing(length, priors),
        (None, _) => sweeping(),
    }
}

fn dividing(length: Duration, priors: Priors) -> Vec<Duration> {
    (1..)
        .map(|beats| (beats, length / beats))
        .take_while(|(_, period)| *period >= briskest())
        .filter(|(_, period)| *period <= slowest())
        .filter(|(beats, _)| priors.whole_bars(*beats))
        .map(|(_, period)| period)
        .collect()
}

fn sweeping() -> Vec<Duration> {
    let mut period = briskest();
    let mut swept = Vec::new();

    while period <= slowest() {
        swept.push(period);
        period = period.mul_f64(SWEEP_STEP);
    }

    swept
}

fn frames_in(span: Duration, hop: Duration) -> usize {
    ((span.as_nanos() / hop.as_nanos()) as usize).max(1)
}

fn preference(period: Duration) -> f64 {
    let octaves = (SECONDS_PER_MINUTE / period.as_secs_f64() / PREFERRED).log2() / SPREAD;

    (-0.5 * octaves * octaves).exp()
}

fn bar_phase(envelope: &Envelope, beats: &[Duration], beats_per_bar: usize) -> usize {
    (0..beats_per_bar)
        .max_by(|one, other| {
            accented(envelope, beats, *one, beats_per_bar).total_cmp(&accented(
                envelope,
                beats,
                *other,
                beats_per_bar,
            ))
        })
        .unwrap_or_default()
}

fn accented(envelope: &Envelope, beats: &[Duration], phase: usize, beats_per_bar: usize) -> f32 {
    beats
        .iter()
        .skip(phase)
        .step_by(beats_per_bar)
        .map(|at| envelope.at(*at))
        .sum()
}

fn briskest() -> Duration {
    Duration::from_secs_f64(SECONDS_PER_MINUTE / FASTEST)
}

fn slowest() -> Duration {
    Duration::from_secs_f64(SECONDS_PER_MINUTE / SLOWEST)
}
