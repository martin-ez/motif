//! Finding the beats of a take, and which of them begin a bar.
//!
//! The facts worth stating are that a click track is found beat for beat with
//! or without what the looper knows, that the length prior puts a whole number
//! of beats in the take, that a bar is four beats until something says
//! otherwise, that a take which speeds up is followed rather than averaged,
//! and that a take with nothing in it yields nothing rather than a grid over
//! silence.

use motif::analysis::{Priors, Tracked, track};
use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE};
use motif::fixtures::{Drift, Recipe, Score, Texture};
use std::time::Duration;

const FOUR_FOUR: usize = 4;
const THREE_FOUR: usize = 3;
const BARS: usize = 4;
const MODERATE: f64 = 120.0;
const BRISK: f64 = 150.0;
const SHARP: f64 = 1.0;
const ONE_TO_THE_BEAT: usize = 1;
const NONE_UNSOUNDED: f64 = 0.0;
const ON_THE_BEAT: f64 = 0.0;
const TWICE_TO_THE_BEAT: usize = 2;
const HALF_OFF_THE_BEAT: f64 = 0.5;
const A_BAR_OF_SILENCE: Duration = Duration::from_secs(2);

fn clicks(tempo: f64, meter: usize, drift: Drift) -> Fixture {
    struck(tempo, meter, drift, ONE_TO_THE_BEAT, ON_THE_BEAT)
}

fn subdivided(tempo: f64, meter: usize) -> Fixture {
    struck(tempo, meter, Drift::Steady, TWICE_TO_THE_BEAT, ON_THE_BEAT)
}

fn syncopated(tempo: f64, meter: usize) -> Fixture {
    struck(
        tempo,
        meter,
        Drift::Steady,
        ONE_TO_THE_BEAT,
        HALF_OFF_THE_BEAT,
    )
}

fn struck(tempo: f64, meter: usize, drift: Drift, density: usize, syncopation: f64) -> Fixture {
    synth::rendered(
        "tracked",
        Recipe {
            tempo,
            meter,
            bars: BARS,
            drift,
            texture: Texture::Percussion {
                sharpness: SHARP,
                density,
                dropout: NONE_UNSOUNDED,
                syncopation,
            },
        },
    )
}

fn heard(fixture: &Fixture) -> impl Iterator<Item = f32> + '_ {
    fixture
        .samples()
        .iter()
        .map(|sample| f32::from(*sample) / f32::from(i8::MAX))
}

/// What the looper knows: the take runs to one interval past its last beat,
/// because the loop wraps to bar one there.
fn take(fixture: &Fixture) -> Duration {
    let beats = fixture.beats();
    let last = beats.last().expect("the fixture has beats").at;

    last + last / (beats.len() as u32 - 1)
}

fn tracked(fixture: &Fixture, priors: Priors) -> Tracked {
    track(heard(fixture), SAMPLE_RATE, priors)
}

fn annotated(fixture: &Fixture) -> Vec<Duration> {
    fixture.beats().iter().map(|beat| beat.at).collect()
}

fn scored(fixture: &Fixture, found: &Tracked) -> Score {
    Score::of(&annotated(fixture), found.beats())
}

fn intervals(found: &Tracked) -> Vec<Duration> {
    found
        .beats()
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect()
}

#[test]
fn a_click_track_is_found_beat_for_beat() {
    let fixture = clicks(MODERATE, FOUR_FOUR, Drift::Steady);
    let found = tracked(
        &fixture,
        Priors::of_take(take(&fixture)).with_meter(FOUR_FOUR),
    );

    assert_eq!(
        scored(&fixture, &found).f1(),
        1.0,
        "{}",
        scored(&fixture, &found)
    );
}

#[test]
fn a_click_track_is_found_beat_for_beat_knowing_nothing_about_it() {
    let fixture = clicks(MODERATE, FOUR_FOUR, Drift::Steady);
    let found = tracked(&fixture, Priors::blind());

    assert_eq!(
        scored(&fixture, &found).f1(),
        1.0,
        "{}",
        scored(&fixture, &found)
    );
}

#[test]
fn the_take_length_prior_puts_a_whole_number_of_beats_in_the_take() {
    let fixture = clicks(BRISK, FOUR_FOUR, Drift::Steady);
    let found = tracked(&fixture, Priors::of_take(take(&fixture)));

    assert_eq!(found.beats().len(), BARS * FOUR_FOUR);
}

#[test]
fn beats_come_back_in_the_order_they_fall() {
    let fixture = clicks(MODERATE, FOUR_FOUR, Drift::Steady);
    let found = tracked(&fixture, Priors::of_take(take(&fixture)));

    assert!(
        found.beats().windows(2).all(|pair| pair[0] < pair[1]),
        "{:?} does not rise",
        found.beats()
    );
}

#[test]
fn a_waltz_starts_a_bar_every_third_beat() {
    let fixture = clicks(BRISK, THREE_FOUR, Drift::Steady);
    let found = tracked(
        &fixture,
        Priors::of_take(take(&fixture)).with_meter(THREE_FOUR),
    );
    let bars: Vec<Duration> = fixture
        .beats()
        .iter()
        .filter(|beat| beat.is_downbeat)
        .map(|beat| beat.at)
        .collect();

    assert_eq!(found.beats_per_bar(), THREE_FOUR);
    assert_eq!(
        Score::of(&bars, &found.downbeats().collect::<Vec<_>>()).f1(),
        1.0
    );
}

#[test]
fn every_downbeat_is_one_of_the_beats() {
    let fixture = clicks(MODERATE, FOUR_FOUR, Drift::Steady);
    let found = tracked(
        &fixture,
        Priors::of_take(take(&fixture)).with_meter(FOUR_FOUR),
    );

    assert!(
        found.downbeats().all(|at| found.beats().contains(&at)),
        "a downbeat fell where no beat did"
    );
    assert_eq!(found.downbeats().count(), BARS);
}

#[test]
fn a_bar_is_four_beats_until_something_says_otherwise() {
    let fixture = clicks(BRISK, THREE_FOUR, Drift::Steady);
    let found = tracked(&fixture, Priors::of_take(take(&fixture)));

    assert_eq!(found.beats_per_bar(), Priors::ASSUMED_BAR);
    assert_eq!(Priors::ASSUMED_BAR, FOUR_FOUR);
}

#[test]
fn a_bar_of_no_beats_is_not_a_bar() {
    let fixture = clicks(MODERATE, FOUR_FOUR, Drift::Steady);
    let found = tracked(&fixture, Priors::blind().with_meter(0));

    assert_eq!(found.beats_per_bar(), Priors::ASSUMED_BAR);
}

#[test]
fn a_take_with_nothing_in_it_has_no_beats() {
    let silence = vec![0.0; (A_BAR_OF_SILENCE.as_secs_f64() * f64::from(SAMPLE_RATE)) as usize];
    let found = track(silence, SAMPLE_RATE, Priors::of_take(A_BAR_OF_SILENCE));

    assert_eq!(found.beats(), &[] as &[Duration]);
    assert_eq!(found.downbeats().count(), 0);
}

#[test]
fn a_take_that_speeds_up_is_followed_rather_than_averaged() {
    let fixture = clicks(100.0, FOUR_FOUR, Drift::Ramp { to: 140.0 });
    let found = tracked(&fixture, Priors::of_take(take(&fixture)));
    let intervals = intervals(&found);
    let (first, last) = (intervals[0], intervals[intervals.len() - 1]);

    assert!(
        last < first,
        "the grid did not speed up: {first:?} then {last:?}"
    );
}

#[test]
fn the_bar_count_prior_pins_a_take_that_sounds_twice_to_the_beat() {
    let fixture = subdivided(80.0, FOUR_FOUR);
    let told = Priors::of_take(take(&fixture)).with_meter(FOUR_FOUR);
    let found = tracked(&fixture, told.with_bars(BARS));

    assert_eq!(found.beats().len(), BARS * FOUR_FOUR);
    assert_eq!(
        scored(&fixture, &found).f1(),
        1.0,
        "{}",
        scored(&fixture, &found)
    );
}

#[test]
fn a_take_of_no_bars_is_not_a_take() {
    let fixture = clicks(MODERATE, FOUR_FOUR, Drift::Steady);
    let told = Priors::of_take(take(&fixture)).with_meter(FOUR_FOUR);
    let found = tracked(&fixture, told.with_bars(0));

    assert_eq!(found.beats(), tracked(&fixture, told).beats());
}

#[test]
fn knowing_the_meter_puts_a_whole_number_of_bars_in_the_take() {
    let fixture = clicks(BRISK, THREE_FOUR, Drift::Steady);
    let found = tracked(
        &fixture,
        Priors::of_take(take(&fixture)).with_meter(THREE_FOUR),
    );

    assert_eq!(
        found.beats().len() % THREE_FOUR,
        0,
        "{} beats",
        found.beats().len()
    );
}

#[test]
fn a_take_played_off_the_beat_keeps_its_pulse() {
    let fixture = syncopated(MODERATE, FOUR_FOUR);
    let found = tracked(
        &fixture,
        Priors::of_take(take(&fixture))
            .with_meter(FOUR_FOUR)
            .with_bars(BARS),
    );

    assert_eq!(
        scored(&fixture, &found).f1(),
        1.0,
        "{}",
        scored(&fixture, &found)
    );
}

#[test]
fn a_steady_take_does_not_wander_off_its_pulse() {
    let fixture = struck(
        80.0,
        FOUR_FOUR,
        Drift::Steady,
        TWICE_TO_THE_BEAT,
        HALF_OFF_THE_BEAT,
    );
    let found = tracked(
        &fixture,
        Priors::of_take(take(&fixture))
            .with_meter(FOUR_FOUR)
            .with_bars(BARS),
    );

    assert_eq!(
        scored(&fixture, &found).f1(),
        1.0,
        "{}",
        scored(&fixture, &found)
    );
}

#[test]
fn a_bar_begins_where_the_take_is_accented_rather_than_where_it_starts() {
    const A_BEAT: usize = SAMPLE_RATE as usize;
    const STRUCK_FOR: usize = 100;
    const ACCENTED: f32 = 0.9;
    const UNACCENTED: f32 = 0.3;
    const SECOND_OF_THE_BAR: usize = 1;
    const EIGHT_BEATS: Duration = Duration::from_secs(8);

    let played = |frame: usize| {
        let accented = (frame / A_BEAT) % FOUR_FOUR == SECOND_OF_THE_BAR;
        match (frame % A_BEAT < STRUCK_FOR, accented) {
            (false, _) => 0.0,
            (true, true) => ACCENTED,
            (true, false) => UNACCENTED,
        }
    };
    let found = track(
        (0..A_BEAT * 8).map(played),
        SAMPLE_RATE,
        Priors::of_take(EIGHT_BEATS)
            .with_meter(FOUR_FOUR)
            .with_bars(2),
    );

    assert_eq!(
        found.downbeats().next(),
        Some(Duration::from_secs(SECOND_OF_THE_BAR as u64)),
        "bars began at {:?}",
        found.downbeats().collect::<Vec<_>>()
    );
}

#[test]
fn a_take_off_the_preferred_tempo_is_not_tracked_at_twice_its_pulse() {
    const UNDER_PREFERRED: f64 = 100.0;

    let fixture = clicks(UNDER_PREFERRED, FOUR_FOUR, Drift::Steady);
    let found = tracked(&fixture, Priors::blind());

    assert_eq!(
        scored(&fixture, &found).f1(),
        1.0,
        "{}",
        scored(&fixture, &found)
    );
}

#[test]
fn a_take_played_off_the_beat_is_not_counted_in_its_subdivisions() {
    const BETWEEN_THE_TWO: f64 = 130.0;

    let fixture = syncopated(BETWEEN_THE_TWO, FOUR_FOUR);
    let found = tracked(&fixture, Priors::of_take(take(&fixture)));

    assert_eq!(found.beats().len(), BARS * FOUR_FOUR);
}
