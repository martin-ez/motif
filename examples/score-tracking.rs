//! Score the beat tracker over a drawn fixture set, once for each thing the
//! looper can tell it.
//!
//! ```sh
//! cargo run --release --example score-tracking
//! ```
//!
//! What ranks the approaches is not the aggregate but where each one loses, so
//! this prints the bands as well: the same set broken down by the parameter
//! each fixture was rendered from. The set is drawn from the evaluation seed,
//! which is held back from development so the figure is not the one the
//! tracker was tuned to.

use motif::analysis::{Priors, track};
use motif::fixtures::harness::{self, Report, Target};
use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE};
use motif::fixtures::{Axis, Score};
use std::time::Duration;

const SET: usize = 48;

type Told = fn(&Fixture) -> Priors;

fn heard(fixture: &Fixture) -> impl Iterator<Item = f32> + '_ {
    fixture
        .samples()
        .iter()
        .map(|sample| f32::from(*sample) / f32::from(i8::MAX))
}

fn take(fixture: &Fixture) -> Duration {
    let beats = fixture.beats();
    let last = beats.last().expect("the fixture has beats").at;

    last + last / (beats.len() as u32 - 1)
}

fn scored(set: &[Fixture], target: Target, priors: impl Fn(&Fixture) -> Priors) -> Report<Score> {
    harness::measure_rendered(set, target, |fixture| {
        let found = track(heard(fixture), SAMPLE_RATE, priors(fixture));

        match target {
            Target::Beats => found.beats().to_vec(),
            Target::Downbeats => found.downbeats().collect(),
        }
    })
}

fn banded(report: &Report<Score>) {
    for axis in Axis::ALL {
        println!("  {}", axis.named());
        for band in report.by(axis) {
            println!("    {band}");
        }
    }
}

fn main() {
    let set = synth::drawn(synth::EVALUATION, SET);
    let told: [(&str, Told); 4] = [
        ("blind", |_| Priors::blind()),
        ("length", |fixture| Priors::of_take(take(fixture))),
        ("length + meter", |fixture| {
            Priors::of_take(take(fixture)).with_meter(fixture.recipe().meter)
        }),
        ("length + meter + bars", |fixture| {
            Priors::of_take(take(fixture))
                .with_meter(fixture.recipe().meter)
                .with_bars(fixture.recipe().bars)
        }),
    ];

    println!("{SET} fixtures drawn from the evaluation seed\n");
    for (named, priors) in told {
        let beats = scored(&set, Target::Beats, priors);
        let downbeats = scored(&set, Target::Downbeats, priors);

        println!(
            "{named:<22}beats F1 {:.3}   downbeats F1 {:.3}   headroom {:.1?}",
            beats.mean(),
            downbeats.mean(),
            beats.headroom().min(downbeats.headroom()),
        );
    }

    let (named, priors) = told[told.len() - 1];
    let beats = scored(&set, Target::Beats, priors);
    let downbeats = scored(&set, Target::Downbeats, priors);
    println!("\n{named}, beats");
    banded(&beats);
    println!("\n{named}, downbeats");
    banded(&downbeats);
}
