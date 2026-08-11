//! What a bar count is worth to the beat tracker, and what guessing one costs.
//!
//! `#362` asks how the looper knows a take's bar count, and the answer rests on
//! a number rather than a preference: the player states it if it is worth
//! stating. So the facts worth stating are what a stated count buys over the
//! meter alone, and that a guessed one is worse than not counting at all —
//! which is why an uncounted take carries no count instead of a likely one.
//!
//! Scored over takes whose bar count varies, since a set that runs the same
//! number of bars throughout cannot tell a stated count from a lucky guess.

use motif::analysis::{Priors, track};
use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE};
use motif::fixtures::{Drift, Recipe, Score, Texture};
use motif::seq::Bars;
use std::time::Duration;

/// The count an assumption would reach for: four bars of four beats, which is
/// what most loops are and what the rest are not.
const A_GUESS: usize = 4;

const BAR_COUNTS: [usize; 4] = [3, 5, 6, 8];
const METERS: [usize; 3] = [3, 4, 5];
const TEMPI: [f64; 2] = [100.0, 140.0];
const RAMP_REACH: f64 = 1.4;
const RUBATO_PULL: f64 = 0.13;
const SHARPNESSES: [f64; 2] = [1.0, 0.7];
const DENSITIES: [usize; 2] = [1, 2];
const DROPOUTS: [f64; 2] = [0.0, 0.3];
const SYNCOPATIONS: [f64; 2] = [0.0, 0.25];

/// Takes across the parameters a drawn set samples, with the bar count varied
/// through them rather than held at one value.
fn varied() -> Vec<(Fixture, Bars)> {
    let mut set = Vec::new();

    for (index, &count) in BAR_COUNTS.iter().enumerate() {
        for (turn, &beats_each) in METERS.iter().enumerate() {
            let tempo = TEMPI[turn % TEMPI.len()];

            for drift in [
                Drift::Steady,
                Drift::Ramp {
                    to: tempo * RAMP_REACH,
                },
                Drift::Rubato { pull: RUBATO_PULL },
            ] {
                let bars = Bars::of(count, beats_each).expect("the counts here are counts");
                let recipe = Recipe {
                    tempo,
                    meter: beats_each,
                    bars: count,
                    drift,
                    texture: Texture::Percussion {
                        sharpness: SHARPNESSES[index % SHARPNESSES.len()],
                        density: DENSITIES[index % DENSITIES.len()],
                        dropout: DROPOUTS[(index + turn) % DROPOUTS.len()],
                        syncopation: SYNCOPATIONS[(index + turn) % SYNCOPATIONS.len()],
                    },
                };

                set.push((synth::rendered("counted", recipe), bars));
            }
        }
    }

    set
}

fn heard(fixture: &Fixture) -> impl Iterator<Item = f32> + '_ {
    fixture
        .samples()
        .iter()
        .map(|sample| f32::from(*sample) / f32::from(i8::MAX))
}

/// How long the take runs: to one interval past its last beat, where the loop
/// wraps back to bar one.
fn take(fixture: &Fixture) -> Duration {
    let beats = fixture.beats();
    let last = beats.len() - 1;

    beats[last].at + (beats[last].at - beats[last - 1].at)
}

fn downbeats(fixture: &Fixture) -> Vec<Duration> {
    fixture
        .beats()
        .iter()
        .filter(|beat| beat.is_downbeat)
        .map(|beat| beat.at)
        .collect()
}

/// The mean downbeat F1 over the set, of grids tracked under the priors
/// `supplied` builds from what each take's player stated.
fn scored(supplied: impl Fn(Duration, Bars) -> Priors) -> f64 {
    let set = varied();
    let total: f64 = set
        .iter()
        .map(|(fixture, bars)| {
            let found = track(heard(fixture), SAMPLE_RATE, supplied(take(fixture), *bars));
            let placed: Vec<Duration> = found.downbeats().collect();

            Score::of(&downbeats(fixture), &placed).f1()
        })
        .sum();

    total / set.len() as f64
}

fn stating_the_meter(length: Duration, bars: Bars) -> Priors {
    Priors::of_take(length).with_meter(bars.beats_each())
}

fn stating_the_count(length: Duration, bars: Bars) -> Priors {
    stating_the_meter(length, bars).with_bars(bars.count())
}

fn guessing_the_count(length: Duration, bars: Bars) -> Priors {
    stating_the_meter(length, bars).with_bars(A_GUESS)
}

/// How much better a stated count has to score than the meter alone before it
/// is worth a control the player has to reach for.
const WORTH_STATING: f64 = 0.15;

#[test]
fn a_stated_bar_count_places_downbeats_better_than_the_meter_alone() {
    let alone = scored(stating_the_meter);
    let stated = scored(stating_the_count);

    assert!(
        stated > alone + WORTH_STATING,
        "a stated count scores {stated:.2} against {alone:.2} for the meter alone"
    );
}

#[test]
fn a_guessed_bar_count_places_downbeats_worse_than_not_counting() {
    let alone = scored(stating_the_meter);
    let guessed = scored(guessing_the_count);

    assert!(
        guessed < alone,
        "a guessed count scores {guessed:.2} against {alone:.2} for not counting"
    );
}

#[test]
fn a_stated_bar_count_puts_exactly_that_many_beats_in_the_take() {
    let (fixture, bars) = varied().remove(0);

    let found = track(
        heard(&fixture),
        SAMPLE_RATE,
        stating_the_count(take(&fixture), bars),
    );

    assert_eq!(found.beats().len(), bars.count() * bars.beats_each());
}
