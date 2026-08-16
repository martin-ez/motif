//! Score the chord pass over a drawn harmonic set, at each level two chord
//! labels can be said to agree at.
//!
//! ```sh
//! cargo run --release --example score-chords
//! ```
//!
//! The pass is the crude one, so the figure is a floor rather than a result:
//! it is the number a spectral front end worth the name has to beat. Accuracies
//! at different comparison levels are not comparable, so all three are printed
//! and each is quoted with its level. The bands say where it lost, and the set
//! is drawn from the evaluation seed, held back from development.

use motif::analysis::{Priors, chords, track};
use motif::fixtures::harness::{self, Report};
use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE};
use motif::fixtures::{Agreement, Axis, Comparison};

const SET: usize = 24;

const LEVELS: [Comparison; 3] = [Comparison::Root, Comparison::Thirds, Comparison::Sevenths];

fn heard(fixture: &Fixture) -> Vec<f32> {
    fixture
        .samples()
        .iter()
        .map(|sample| f32::from(*sample) / f32::from(i8::MAX))
        .collect()
}

/// The priors the looper hands the tracker: what the player counted the take
/// in, which for a fixture is what it was rendered from.
fn told(fixture: &Fixture) -> Priors {
    let beats = fixture.beats();
    let last = beats.last().expect("the fixture has beats").at;

    Priors::of_take(last + last / (beats.len() as u32 - 1))
        .with_meter(fixture.recipe().meter)
        .with_bars(fixture.recipe().bars)
}

fn scored(set: &[Fixture], comparison: Comparison) -> Report<Agreement> {
    harness::measure_rendered_chords(set, comparison, |fixture| {
        let played = heard(fixture);
        let grid = track(played.iter().copied(), SAMPLE_RATE, told(fixture));

        chords(&played, SAMPLE_RATE, grid.beats())
    })
}

fn banded(report: &Report<Agreement>) {
    for axis in [Axis::Tempo, Axis::Meter, Axis::Drift] {
        println!("  {}", axis.named());
        for band in report.by(axis) {
            println!("    {band}");
        }
    }
}

fn main() {
    let set = synth::drawn_chords(synth::EVALUATION, SET);

    println!("{SET} harmonic fixtures drawn from the evaluation seed\n");
    for comparison in LEVELS {
        let report = scored(&set, comparison);

        println!(
            "{comparison:<10?} accuracy {:.3}   headroom {:.1?}",
            report.mean(),
            report.headroom()
        );
    }

    let report = scored(&set, Comparison::Root);
    println!("\nroot");
    banded(&report);
}
