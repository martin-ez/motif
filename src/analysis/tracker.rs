//! Placing a grid of beats over a take, and finding which of them begin a bar.
//!
//! A pulse is chosen by how much of the take's onset strength the grid it
//! implies explains, and then each beat is placed on the strongest onset near
//! where the pulse expects it, which is what lets the grid follow a take that
//! speeds up or breathes rather than averaging over it.

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
const WANDER: f64 = 0.25;
const PHASE_STEP: f64 = 0.125;
const FOLLOW: f64 = 0.5;
const SWEEP_STEP: f64 = 1.03;
const SECONDS_PER_MINUTE: f64 = 60.0;

/// What a manual looper knows about a take that a beat tracker does not.
///
/// The player closed the loop, so its length is a fact rather than an
/// estimate, and a pulse has to divide it into a whole number of beats. How
/// many of those go to a bar is the other half, and nothing in the engine
/// carries it yet, so it is optional and [`ASSUMED_BAR`](Self::ASSUMED_BAR)
/// stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priors {
    length: Option<Duration>,
    beats_per_bar: Option<usize>,
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
        }
    }

    /// Knowing that the take is `length` long and repeats there.
    pub fn of_take(length: Duration) -> Self {
        Self {
            length: Some(length),
            beats_per_bar: None,
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

    fn bar(&self) -> usize {
        self.beats_per_bar.unwrap_or(Self::ASSUMED_BAR)
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
/// Where two grids explain the audio equally well — one at twice the rate of
/// the other — the tie is broken by a log-Gaussian preference for 120 BPM nine
/// tenths of an octave wide, as Ellis's dynamic-programming beat tracker breaks
/// it. Without one, nothing prefers a pulse to its own double.
///
/// A take whose audio explains no grid at all yields no beats, rather than a
/// grid laid over silence.
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
pub fn track(
    samples: impl IntoIterator<Item = f32>,
    sample_rate: u32,
    priors: Priors,
) -> Tracked {
    let envelope = Envelope::of(samples, sample_rate);
    let beats = strongest_grid(&envelope, priors).unwrap_or_default();
    let downbeat = bar_phase(&envelope, &beats, priors.bar());

    Tracked {
        beats,
        beats_per_bar: priors.bar(),
        downbeat,
    }
}

struct Candidate {
    period: Duration,
    beats: Option<usize>,
}

fn strongest_grid(envelope: &Envelope, priors: Priors) -> Option<Vec<Duration>> {
    let mut strongest: Option<(f64, Vec<Duration>)> = None;

    for candidate in candidates(priors) {
        let step = candidate.period.mul_f64(PHASE_STEP);
        let mut phase = Duration::ZERO;
        while phase < candidate.period {
            let (beats, explained) = place(envelope, &candidate, phase);
            let score = explained * preference(candidate.period);
            if score > strongest.as_ref().map_or(0.0, |(best, _)| *best) {
                strongest = Some((score, beats));
            }
            phase += step;
        }
    }

    strongest.map(|(_, beats)| beats)
}

fn candidates(priors: Priors) -> Vec<Candidate> {
    match priors.length {
        Some(length) => dividing(length),
        None => sweeping(),
    }
}

fn dividing(length: Duration) -> Vec<Candidate> {
    (1..)
        .map(|beats| (beats, length / beats))
        .take_while(|(_, period)| *period >= briskest())
        .filter(|(_, period)| *period <= slowest())
        .map(|(beats, period)| Candidate {
            period,
            beats: Some(beats as usize),
        })
        .collect()
}

fn sweeping() -> Vec<Candidate> {
    let mut period = briskest();
    let mut swept = Vec::new();

    while period <= slowest() {
        swept.push(Candidate {
            period,
            beats: None,
        });
        period = period.mul_f64(SWEEP_STEP);
    }

    swept
}

fn place(envelope: &Envelope, candidate: &Candidate, phase: Duration) -> (Vec<Duration>, f64) {
    let reach = candidate.period.mul_f64(WANDER);
    let mut beats: Vec<Duration> = Vec::new();
    let mut explained = 0.0;
    let mut running = candidate.period;
    let mut expected = phase;

    while expected <= envelope.span() && candidate.beats.is_none_or(|count| beats.len() < count) {
        let (at, strength) = strongest_near(envelope, expected, reach);
        if let Some(last) = beats.last() {
            running = following(running, at - *last);
        }
        beats.push(at);
        explained += f64::from(strength);
        expected = at + running;
    }

    (beats, explained)
}

fn following(running: Duration, taken: Duration) -> Duration {
    let followed = running.mul_f64(1.0 - FOLLOW) + taken.mul_f64(FOLLOW);

    followed.clamp(briskest(), slowest())
}

fn strongest_near(envelope: &Envelope, expected: Duration, reach: Duration) -> (Duration, f32) {
    let hop = envelope.hop();
    let first = frames_in(expected.saturating_sub(reach), hop);
    let last = frames_in(expected + reach, hop);
    let mut strongest = (expected, envelope.at(expected));

    for frame in first..=last {
        let at = hop * frame;
        let strength = envelope.at(at);
        if strength > strongest.1 {
            strongest = (at, strength);
        }
    }

    strongest
}

fn frames_in(span: Duration, hop: Duration) -> u32 {
    (span.as_nanos() / hop.as_nanos()) as u32
}

fn preference(period: Duration) -> f64 {
    let octaves = (SECONDS_PER_MINUTE / period.as_secs_f64() / PREFERRED).log2() / SPREAD;

    (-0.5 * octaves * octaves).exp()
}

fn bar_phase(envelope: &Envelope, beats: &[Duration], beats_per_bar: usize) -> usize {
    (0..beats_per_bar)
        .max_by(|one, other| {
            accented(envelope, beats, *one, beats_per_bar)
                .total_cmp(&accented(envelope, beats, *other, beats_per_bar))
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
