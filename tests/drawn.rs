//! Fixtures rendered from a seed rather than read off disk: what a recipe puts
//! in one, and what a set of them covers.

use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE, Voice};
use motif::fixtures::{Annotation, Axis, Drift, Recipe, Texture};
use std::time::Duration;

const A_SEED: u32 = 0x2f7a_1c05;

fn clicks(density: usize, dropout: f64, syncopation: f64) -> Recipe {
    Recipe {
        tempo: 120.0,
        meter: 4,
        bars: 4,
        drift: Drift::Steady,
        texture: Texture::Percussion {
            sharpness: 1.0,
            density,
            dropout,
            syncopation,
        },
    }
}

fn plain() -> Recipe {
    clicks(1, 0.0, 0.0)
}

fn ticks(fixture: &Fixture) -> Vec<Duration> {
    fixture
        .onsets()
        .iter()
        .filter(|onset| onset.voice == Voice::Tick)
        .map(|onset| onset.at)
        .collect()
}

fn accents(fixture: &Fixture) -> Vec<Duration> {
    fixture
        .onsets()
        .iter()
        .filter(|onset| onset.voice == Voice::Accent)
        .map(|onset| onset.at)
        .collect()
}

fn intervals(fixture: &Fixture) -> Vec<Duration> {
    fixture
        .beats()
        .windows(2)
        .map(|pair| pair[1].at - pair[0].at)
        .collect()
}

fn peak_over(fixture: &Fixture, span: Duration) -> i16 {
    let frames = (span.as_secs_f64() * f64::from(SAMPLE_RATE)) as usize;
    fixture.samples()[..frames]
        .iter()
        .map(|sample| i16::from(*sample).abs())
        .max()
        .unwrap_or(0)
}

#[test]
fn a_recipe_lays_out_the_beats_its_meter_and_bars_ask_for() {
    let fixture = synth::rendered("laid-out", plain());

    assert_eq!(fixture.beats().len(), 16);
    assert_eq!(fixture.beats().iter().filter(|b| b.is_downbeat).count(), 4);
}

#[test]
fn a_recipe_is_carried_by_the_fixture_it_rendered() {
    let recipe = clicks(2, 0.15, 0.25);

    assert_eq!(*synth::rendered("carried", recipe).recipe(), recipe);
}

#[test]
fn a_steady_recipe_holds_one_interval_throughout() {
    let intervals = intervals(&synth::rendered("held", plain()));
    let frame = Duration::from_nanos(u64::from(1_000_000_000 / SAMPLE_RATE));

    let longest = intervals.iter().max().copied().unwrap_or_default();
    let shortest = intervals.iter().min().copied().unwrap_or_default();
    assert!(longest - shortest <= frame, "{intervals:?}");
}

#[test]
fn a_ramped_recipe_shortens_every_interval() {
    let recipe = Recipe {
        drift: Drift::Ramp { to: 160.0 },
        ..plain()
    };
    let intervals = intervals(&synth::rendered("ramped", recipe));

    assert!(
        intervals.windows(2).all(|pair| pair[1] < pair[0]),
        "{intervals:?}"
    );
}

#[test]
fn a_rubato_recipe_both_pushes_and_pulls() {
    let recipe = Recipe {
        drift: Drift::Rubato { pull: 0.13 },
        ..plain()
    };
    let intervals = intervals(&synth::rendered("strayed", recipe));

    assert!(
        intervals.windows(2).any(|pair| pair[1] < pair[0]),
        "{intervals:?}"
    );
    assert!(
        intervals.windows(2).any(|pair| pair[1] > pair[0]),
        "{intervals:?}"
    );
}

#[test]
fn a_ramped_recipe_of_a_single_beat_lays_out_that_beat() {
    let recipe = Recipe {
        meter: 1,
        bars: 1,
        drift: Drift::Ramp { to: 160.0 },
        ..plain()
    };
    let fixture = synth::rendered("lone", recipe);

    assert_eq!(fixture.beats().len(), 1);
}

#[test]
fn a_two_beat_ramp_lays_its_second_beat_a_beat_later() {
    let recipe = Recipe {
        meter: 2,
        bars: 1,
        drift: Drift::Ramp { to: 160.0 },
        ..plain()
    };
    let fixture = synth::rendered("pair", recipe);
    let beats = fixture.beats();

    assert_eq!(beats[0].at, Duration::ZERO);
    assert_eq!(beats[1].at, Duration::from_millis(500));
}

const EVERY_DRIFT: [Drift; 3] = [
    Drift::Steady,
    Drift::Ramp { to: 160.0 },
    Drift::Rubato { pull: 0.13 },
];

fn of_no_bars(drift: Drift) -> Recipe {
    Recipe {
        bars: 0,
        drift,
        ..plain()
    }
}

#[test]
fn a_recipe_of_no_bars_lays_out_no_beats_under_every_drift() {
    for drift in EVERY_DRIFT {
        let fixture = synth::rendered("empty", of_no_bars(drift));

        assert!(
            fixture.beats().is_empty(),
            "{drift:?} laid out {:?}",
            fixture.beats()
        );
    }
}

#[test]
fn a_recipe_of_no_bars_sounds_nothing_under_every_drift() {
    for drift in EVERY_DRIFT {
        let fixture = synth::rendered("silent", of_no_bars(drift));

        assert!(
            fixture.onsets().is_empty(),
            "{drift:?} sounded {:?}",
            fixture.onsets()
        );
    }
}

const NO_TEMPO: [f64; 4] = [0.0, -120.0, f64::NAN, f64::INFINITY];

fn at_no_tempo(tempo: f64, drift: Drift) -> Recipe {
    Recipe {
        tempo,
        drift,
        ..plain()
    }
}

#[test]
fn a_recipe_of_no_tempo_lays_out_no_beats_under_every_drift() {
    for drift in EVERY_DRIFT {
        for tempo in NO_TEMPO {
            let fixture = synth::rendered("tempoless", at_no_tempo(tempo, drift));

            assert!(
                fixture.beats().is_empty(),
                "{drift:?} at {tempo} laid out {:?}",
                fixture.beats()
            );
        }
    }
}

#[test]
fn a_recipe_of_no_tempo_sounds_nothing_under_every_drift() {
    for drift in EVERY_DRIFT {
        for tempo in NO_TEMPO {
            let fixture = synth::rendered("mute", at_no_tempo(tempo, drift));

            assert!(
                fixture.onsets().is_empty(),
                "{drift:?} at {tempo} sounded {:?}",
                fixture.onsets()
            );
        }
    }
}

const GLACIAL: f64 = 1e-6;

#[test]
#[should_panic(expected = "longer than rendering will build")]
fn a_recipe_too_slow_to_render_says_so_rather_than_overflowing() {
    synth::rendered(
        "glacial",
        Recipe {
            tempo: GLACIAL,
            ..plain()
        },
    );
}

#[test]
fn an_unsyncopated_recipe_sounds_every_onset_on_a_beat() {
    let fixture = synth::rendered("on-the-beat", plain());
    let beats: Vec<Duration> = fixture.beats().iter().map(|beat| beat.at).collect();

    for onset in fixture.onsets() {
        assert!(beats.contains(&onset.at), "an onset fell at {:?}", onset.at);
    }
}

#[test]
fn a_fully_syncopated_recipe_sounds_every_tick_between_the_beats() {
    let fixture = synth::rendered("between", clicks(1, 0.0, 1.0));
    let beats: Vec<Duration> = fixture.beats().iter().map(|beat| beat.at).collect();

    assert!(!ticks(&fixture).is_empty());
    for at in ticks(&fixture) {
        assert!(!beats.contains(&at), "a tick fell on the beat at {at:?}");
    }
}

#[test]
fn a_syncopated_tick_falls_midway_between_the_beats_it_sits_between() {
    let fixture = synth::rendered("midway", clicks(1, 0.0, 1.0));
    let beats = fixture.beats();
    let frame = Duration::from_nanos(u64::from(1_000_000_000 / SAMPLE_RATE));

    for at in ticks(&fixture) {
        let Some(before) = beats.iter().rev().find(|beat| beat.at < at) else {
            continue;
        };
        let Some(after) = beats.iter().find(|beat| beat.at > at) else {
            continue;
        };

        assert!((at - before.at).abs_diff(after.at - at) <= frame, "{at:?}");
    }
}

#[test]
fn a_denser_recipe_puts_more_onsets_over_the_same_beats() {
    let sparse = synth::rendered("dense", clicks(1, 0.0, 0.0));
    let dense = synth::rendered("dense", clicks(2, 0.0, 0.0));

    assert_eq!(sparse.beats().len(), dense.beats().len());
    assert!(
        ticks(&dense).len() > ticks(&sparse).len(),
        "{} against {}",
        ticks(&dense).len(),
        ticks(&sparse).len()
    );
}

#[test]
fn a_denser_recipe_subdivides_each_beat_forwards() {
    let fixture = synth::rendered("subdivided", clicks(2, 0.0, 0.0));
    let beats = fixture.beats();
    let last = beats[beats.len() - 1].at;
    let latest = ticks(&fixture)
        .into_iter()
        .max()
        .expect("a dense fixture ticks");

    assert!(
        latest > last,
        "the last tick fell at {latest:?} and the last beat at {last:?}"
    );
}

#[test]
fn a_subdivision_falls_midway_between_the_beat_it_splits_and_the_next() {
    let fixture = synth::rendered("split", clicks(2, 0.0, 0.0));
    let beats = fixture.beats();
    let midway = beats[1].at + (beats[2].at - beats[1].at) / 2;

    assert!(ticks(&fixture).contains(&midway), "no tick at {midway:?}");
}

#[test]
fn dropping_every_beat_leaves_only_the_accents_that_mark_the_bars() {
    let fixture = synth::rendered("dropped", clicks(1, 1.0, 0.0));

    assert!(ticks(&fixture).is_empty());
    assert_eq!(accents(&fixture).len(), 4);
}

fn unsounded(fixture: &Fixture) -> usize {
    fixture
        .beats()
        .iter()
        .filter(|beat| !fixture.onsets().iter().any(|onset| onset.at == beat.at))
        .count()
}

fn displaced(fixture: &Fixture) -> usize {
    let beats: Vec<Duration> = fixture.beats().iter().map(|beat| beat.at).collect();

    ticks(fixture)
        .iter()
        .filter(|at| !beats.contains(at))
        .count()
}

#[test]
fn a_dropout_of_a_half_silences_half_the_beats() {
    let fixture = synth::rendered("half-quiet", clicks(1, 0.5, 0.0));

    assert_eq!(unsounded(&fixture), fixture.beats().len() / 2);
}

#[test]
fn a_syncopation_of_a_half_displaces_half_the_beats() {
    let fixture = synth::rendered("half-late", clicks(1, 0.0, 0.5));

    assert_eq!(displaced(&fixture), fixture.beats().len() / 2);
}

#[test]
fn a_syncopation_of_a_quarter_displaces_a_quarter_of_them() {
    let fixture = synth::rendered("quarter-late", clicks(1, 0.0, 0.25));

    assert_eq!(displaced(&fixture), fixture.beats().len() / 4);
}

#[test]
fn syncopation_is_counted_over_the_beats_that_still_sound() {
    let fixture = synth::rendered("late-of-what-is-left", clicks(1, 0.5, 1.0));

    assert_eq!(ticks(&fixture).len(), fixture.beats().len() / 2);
    assert_eq!(displaced(&fixture), ticks(&fixture).len());
}

#[test]
fn a_share_of_none_and_a_share_of_all_are_the_ends_of_the_same_scale() {
    let none = synth::rendered("none-late", clicks(1, 0.0, 0.0));
    let all = synth::rendered("all-late", clicks(1, 0.0, 1.0));

    assert_eq!(displaced(&none), 0);
    assert_eq!(displaced(&all), all.beats().len());
}

#[test]
fn a_downbeat_is_accented_however_much_is_dropped() {
    for dropout in [0.0, 0.5, 1.0] {
        let fixture = synth::rendered("marked", clicks(1, dropout, 0.0));

        for beat in fixture.beats().iter().filter(|beat| beat.is_downbeat) {
            assert!(
                accents(&fixture).contains(&beat.at),
                "{dropout} dropped the bar at {:?}",
                beat.at
            );
        }
    }
}

const SOFTEST_RISE: Duration = Duration::from_millis(20);

fn softly() -> Recipe {
    Recipe {
        texture: Texture::Percussion {
            sharpness: 0.0,
            density: 1,
            dropout: 0.0,
            syncopation: 0.0,
        },
        ..plain()
    }
}

#[test]
fn a_soft_attack_is_audible_while_it_rises_rather_than_silent_until_it_ends() {
    let sharp = synth::rendered("attack", plain());
    let soft = synth::rendered("attack", softly());

    let rising = peak_over(&soft, SOFTEST_RISE);
    let struck = peak_over(&sharp, SOFTEST_RISE);

    assert!(
        rising * 3 > struck,
        "a soft attack peaked at {rising} over its rise where a sharp one peaked at {struck}"
    );
}

#[test]
fn a_softer_attack_takes_longer_to_reach_its_level() {
    let sharp = synth::rendered("attack", clicks(1, 0.0, 0.0));
    let soft = synth::rendered(
        "attack",
        Recipe {
            texture: Texture::Percussion {
                sharpness: 0.0,
                density: 1,
                dropout: 0.0,
                syncopation: 0.0,
            },
            ..plain()
        },
    );
    let opening = Duration::from_millis(5);

    assert!(
        peak_over(&soft, opening) < peak_over(&sharp, opening),
        "{} against {}",
        peak_over(&soft, opening),
        peak_over(&sharp, opening)
    );
}

fn a_click_of(meter: usize, bars: usize, density: usize) -> Recipe {
    Recipe {
        meter,
        bars,
        ..clicks(density, 0.0, 0.0)
    }
}

#[test]
fn a_percussive_recipe_of_a_single_beat_accents_that_beat() {
    let fixture = synth::rendered("lone", a_click_of(1, 1, 1));

    assert_eq!(accents(&fixture), [Duration::ZERO]);
}

#[test]
fn a_beat_with_no_span_after_it_is_not_subdivided() {
    let fixture = synth::rendered("unsubdivided", a_click_of(1, 1, 4));

    assert!(ticks(&fixture).is_empty(), "{:?}", ticks(&fixture));
}

#[test]
fn a_beat_with_a_span_after_it_is_still_subdivided() {
    let fixture = synth::rendered("subdivided", a_click_of(2, 1, 2));

    assert_eq!(ticks(&fixture).len(), 3);
}

fn a_line(bars: usize) -> Recipe {
    Recipe {
        tempo: 120.0,
        meter: 4,
        bars,
        drift: Drift::Steady,
        texture: Texture::Line,
    }
}

fn pitches(fixture: &Fixture) -> Vec<u8> {
    fixture.notes().iter().map(|note| note.pitch).collect()
}

#[test]
fn a_line_shorter_than_the_phrase_plays_what_its_beats_carry() {
    let fixture = synth::rendered("halved", a_line(2));

    assert_eq!(pitches(&fixture), [60, 62, 64, 65, 64, 62]);
}

#[test]
fn a_line_with_the_phrase_in_full_plays_all_of_it() {
    let fixture = synth::rendered("whole", a_line(4));

    assert_eq!(
        pitches(&fixture),
        [60, 62, 64, 65, 64, 62, 67, 65, 64, 62, 60, 55]
    );
}

#[test]
fn a_short_line_holds_no_note_past_the_end_of_its_grid() {
    let fixture = synth::rendered("stopped", a_line(2));
    let beats = fixture.beats();
    let ends = beats[beats.len() - 1].at + (beats[beats.len() - 1].at - beats[beats.len() - 2].at);

    for note in fixture.notes() {
        assert!(note.offset <= ends, "{note:?} runs past {ends:?}");
    }
}

#[test]
fn a_drawn_fixture_describes_the_parameters_it_was_drawn_with() {
    let described = synth::drawn(A_SEED, 1)[0].description().to_owned();

    for axis in Axis::ALL {
        assert!(described.contains(axis.named()), "{described}");
    }
}

#[test]
fn the_same_seed_draws_the_same_fixtures() {
    assert_eq!(synth::drawn(A_SEED, 4), synth::drawn(A_SEED, 4));
}

#[test]
fn a_longer_draw_extends_the_one_before_it_rather_than_replacing_it() {
    let four = synth::drawn(A_SEED, 4);

    assert_eq!(synth::drawn(A_SEED, 6)[..4], four[..]);
}

#[test]
fn two_seeds_draw_different_fixtures() {
    let recipes = |seed| {
        synth::drawn(seed, 8)
            .iter()
            .map(|fixture| *fixture.recipe())
            .collect::<Vec<_>>()
    };

    assert_ne!(recipes(synth::DEVELOPMENT[0]), recipes(synth::EVALUATION));
}

#[test]
fn no_two_development_seeds_draw_the_same_fixtures() {
    for pair in synth::DEVELOPMENT.windows(2) {
        assert_ne!(synth::drawn(pair[0], 4), synth::drawn(pair[1], 4));
    }
}

#[test]
fn the_evaluation_seed_is_held_apart_from_the_development_ones() {
    assert!(!synth::DEVELOPMENT.contains(&synth::EVALUATION));
}

#[test]
fn a_drawn_fixture_is_named_for_the_seed_and_its_place_in_the_set() {
    let names: Vec<_> = synth::drawn(A_SEED, 3)
        .iter()
        .map(|fixture| fixture.name().to_owned())
        .collect();

    assert_eq!(
        names,
        [
            "drawn-2f7a1c05-000",
            "drawn-2f7a1c05-001",
            "drawn-2f7a1c05-002"
        ]
    );
}

#[test]
fn a_drawn_set_carries_more_bars_than_the_committed_one() {
    let bars = |fixture: &Fixture| fixture.recipe().bars;

    assert!(bars(&synth::drawn(A_SEED, 1)[0]) > bars(&synth::set()[0]));
}

#[test]
fn every_drawn_annotation_round_trips_through_the_parser() {
    for fixture in synth::drawn(A_SEED, 8) {
        let annotation: Annotation = fixture
            .annotation_text()
            .parse()
            .unwrap_or_else(|error| panic!("{} annotates cleanly: {error}", fixture.name()));

        assert_eq!(annotation.beats(), fixture.beats(), "{}", fixture.name());
    }
}

#[test]
fn every_drawn_fixture_starts_on_a_downbeat_and_sounds_it() {
    for fixture in synth::drawn(A_SEED, 8) {
        let first = fixture.beats()[0];

        assert!(first.is_downbeat, "{}", fixture.name());
        assert!(accents(&fixture).contains(&first.at), "{}", fixture.name());
    }
}

#[test]
fn no_drawn_sample_clips() {
    for fixture in synth::drawn(A_SEED, 8) {
        let peak = fixture
            .samples()
            .iter()
            .map(|sample| i16::from(*sample).abs())
            .max()
            .unwrap_or(0);

        assert!(
            peak < i16::from(i8::MAX),
            "{} peaked at {peak}",
            fixture.name()
        );
    }
}

const RAMP_GAIN: f64 = 1.4;

#[test]
fn a_drawn_ramp_reaches_a_tempo_a_fixed_share_above_the_one_it_leaves() {
    let mut ramps = 0;

    for fixture in synth::drawn(A_SEED, 24) {
        let Drift::Ramp { to } = fixture.recipe().drift else {
            continue;
        };
        ramps += 1;

        assert!(
            (to - fixture.recipe().tempo * RAMP_GAIN).abs() < 1e-9,
            "{} ramps from {} to {to}",
            fixture.name(),
            fixture.recipe().tempo
        );
    }

    assert!(ramps > 0, "no drawn fixture ramped");
}

#[test]
fn a_drawn_rubato_strays_far_enough_to_be_scored_wrong() {
    let mut strayed = 0;

    for fixture in synth::drawn(A_SEED, 24) {
        let Drift::Rubato { pull } = fixture.recipe().drift else {
            continue;
        };
        strayed += 1;

        assert!(pull > 0.070, "{} pulls only {pull}", fixture.name());
    }

    assert!(strayed > 0, "no drawn fixture strayed");
}

const LEVELS: [(Axis, usize); 7] = [
    (Axis::Tempo, 5),
    (Axis::Meter, 3),
    (Axis::Drift, 3),
    (Axis::Sharpness, 3),
    (Axis::Density, 2),
    (Axis::Dropout, 3),
    (Axis::Syncopation, 3),
];

#[test]
fn a_large_draw_reaches_every_level_of_every_axis() {
    let set = synth::drawn(synth::EVALUATION, 60);

    for (axis, expected) in LEVELS {
        let mut levels: Vec<_> = set
            .iter()
            .filter_map(|fixture| axis.level(fixture.recipe()))
            .collect();
        levels.sort();
        levels.dedup();

        assert_eq!(levels.len(), expected, "{}: {levels:?}", axis.named());
    }
}
