//! The synthetic fixture set: what it covers, what it renders, and that the
//! files checked in are still the ones its generator produces.

use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE, Voice};
use motif::fixtures::{Annotation, Beat};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn named(name: &str) -> Fixture {
    synth::set()
        .into_iter()
        .find(|fixture| fixture.name() == name)
        .unwrap_or_else(|| panic!("the set contains {name}"))
}

fn intervals(fixture: &Fixture) -> Vec<Duration> {
    fixture
        .beats()
        .windows(2)
        .map(|pair| pair[1].at - pair[0].at)
        .collect()
}

fn beats_per_bar(fixture: &Fixture) -> Vec<usize> {
    let mut lengths = Vec::new();
    let mut counted = 0;
    for beat in fixture.beats() {
        if beat.is_downbeat && counted > 0 {
            lengths.push(counted);
            counted = 0;
        }
        counted += 1;
    }
    lengths
}

fn read(name: &str) -> Vec<u8> {
    fs::read(directory().join(name)).unwrap_or_else(|error| panic!("{name} is checked in: {error}"))
}

fn samples(wav: &[u8]) -> Vec<i16> {
    wav[44..]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

fn peak_around(fixture: &Fixture, at: Duration) -> i16 {
    let centre = (at.as_secs_f64() * f64::from(SAMPLE_RATE)) as usize;
    let window = SAMPLE_RATE as usize / 100;
    fixture.samples()[centre..centre + window]
        .iter()
        .map(|sample| sample.abs())
        .max()
        .unwrap_or(0)
}

#[test]
fn the_set_is_not_empty() {
    assert!(!synth::set().is_empty());
}

#[test]
fn fixture_names_are_unique() {
    let mut names: Vec<_> = synth::set().iter().map(|f| f.name().to_owned()).collect();
    names.sort();
    let count = names.len();
    names.dedup();

    assert_eq!(names.len(), count);
}

#[test]
fn every_annotation_round_trips_through_the_parser() {
    for fixture in synth::set() {
        let annotation: Annotation = fixture
            .annotation_text()
            .parse()
            .unwrap_or_else(|error| panic!("{} annotates cleanly: {error}", fixture.name()));

        assert_eq!(annotation.beats(), fixture.beats(), "{}", fixture.name());
    }
}

#[test]
fn every_fixture_is_long_enough_to_track() {
    for fixture in synth::set() {
        assert!(
            fixture.beats().len() >= 8,
            "{} has {} beats",
            fixture.name(),
            fixture.beats().len()
        );
    }
}

#[test]
fn every_beat_lands_on_the_sample_grid() {
    for fixture in synth::set() {
        let frame = u128::from(1_000_000_000 / SAMPLE_RATE);
        for Beat { at, .. } in fixture.beats() {
            assert_eq!(at.as_nanos() % frame, 0, "{} at {at:?}", fixture.name());
        }
    }
}

#[test]
fn every_fixture_starts_on_a_downbeat() {
    for fixture in synth::set() {
        assert!(fixture.beats()[0].is_downbeat, "{}", fixture.name());
    }
}

#[test]
fn the_set_covers_more_than_one_steady_tempo() {
    let frame = Duration::from_nanos(u64::from(1_000_000_000 / SAMPLE_RATE));
    let steady: Vec<_> = synth::set()
        .iter()
        .filter(|fixture| {
            let intervals = intervals(fixture);
            let longest = intervals.iter().max().copied().unwrap_or_default();
            let shortest = intervals.iter().min().copied().unwrap_or_default();
            longest - shortest <= frame
        })
        .map(|fixture| intervals(fixture)[0].as_millis())
        .collect();
    let mut distinct = steady.clone();
    distinct.sort();
    distinct.dedup();

    assert!(distinct.len() >= 3, "steady tempi: {steady:?}");
}

#[test]
fn the_set_covers_three_four_as_well_as_four_four() {
    let metres: Vec<_> = synth::set()
        .iter()
        .flat_map(beats_per_bar)
        .collect();

    assert!(metres.contains(&3), "bar lengths: {metres:?}");
    assert!(metres.contains(&4), "bar lengths: {metres:?}");
}

#[test]
fn a_waltz_is_three_beats_to_the_bar_throughout() {
    assert!(
        beats_per_bar(&named("waltz-150-3-4"))
            .iter()
            .all(|&n| n == 3)
    );
}

#[test]
fn a_tempo_ramp_shortens_every_interval() {
    let intervals = intervals(&named("ramp-100-140-4-4"));

    assert!(
        intervals.windows(2).all(|pair| pair[1] < pair[0]),
        "{intervals:?}"
    );
}

#[test]
fn a_rubato_passage_pushes_and_pulls() {
    let intervals = intervals(&named("rubato-110-4-4"));

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
fn a_rubato_passage_strays_beyond_the_scoring_tolerance() {
    let fixture = named("rubato-110-4-4");
    let beats = fixture.beats();
    let span = (beats[beats.len() - 1].at - beats[0].at).as_secs_f64();
    let average = span / (beats.len() - 1) as f64;

    let furthest = beats
        .iter()
        .enumerate()
        .map(|(index, beat)| {
            (beat.at.as_secs_f64() - beats[0].at.as_secs_f64() - index as f64 * average).abs()
        })
        .fold(0.0_f64, f64::max);

    assert!(furthest > 0.070, "furthest stray was {furthest} s");
}

#[test]
fn a_syncopated_fixture_puts_most_onsets_off_the_beat() {
    let fixture = named("syncopated-120-4-4");
    let on_the_beat = fixture
        .onsets()
        .iter()
        .filter(|onset| fixture.beats().iter().any(|beat| beat.at == onset.at))
        .count();

    assert!(
        on_the_beat * 2 < fixture.onsets().len(),
        "{on_the_beat} of {} onsets were on a beat",
        fixture.onsets().len()
    );
}

#[test]
fn every_downbeat_is_accented() {
    for fixture in synth::set() {
        for beat in fixture.beats().iter().filter(|beat| beat.is_downbeat) {
            assert!(
                fixture
                    .onsets()
                    .iter()
                    .any(|onset| onset.at == beat.at && onset.voice == Voice::Accent),
                "{} at {:?}",
                fixture.name(),
                beat.at
            );
        }
    }
}

#[test]
fn an_onset_puts_energy_where_its_timestamp_says() {
    let fixture = named("steady-120-4-4");
    let onset = fixture.onsets()[1].at;

    assert!(peak_around(&fixture, onset) > 8_000);
}

#[test]
fn the_audio_falls_quiet_between_onsets() {
    let fixture = named("steady-120-4-4");
    let onset = fixture.onsets()[1].at;
    let loudest = peak_around(&fixture, onset);
    let before = peak_around(&fixture, onset - Duration::from_millis(60));

    assert!(
        i32::from(before) * 10 < i32::from(loudest),
        "{before} before an onset of {loudest}"
    );
}

#[test]
fn no_sample_clips() {
    for fixture in synth::set() {
        let peak = fixture.samples().iter().map(|s| s.abs()).max().unwrap_or(0);

        assert!(peak < i16::MAX, "{} peaked at {peak}", fixture.name());
    }
}

#[test]
fn rendering_is_deterministic() {
    let once = named("rubato-110-4-4");
    let again = named("rubato-110-4-4");

    assert_eq!(once.samples(), again.samples());
}

#[test]
fn a_wav_declares_sixteen_bit_mono_at_the_documented_sample_rate() {
    let wav = named("steady-90-4-4").wav_bytes();

    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1);
    assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
    assert_eq!(
        u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
        SAMPLE_RATE
    );
    assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
}

#[test]
fn a_wav_declares_the_size_of_its_sample_data() {
    let fixture = named("steady-90-4-4");
    let wav = fixture.wav_bytes();
    let data = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;

    assert_eq!(&wav[36..40], b"data");
    assert_eq!(data, fixture.samples().len() * 2);
    assert_eq!(wav.len(), 44 + data);
    assert_eq!(
        u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]) as usize,
        wav.len() - 8
    );
}

#[test]
fn every_fixture_is_checked_in_beside_its_annotation() {
    for fixture in synth::set() {
        let audio = read(&format!("{}.wav", fixture.name()));
        let annotation = read(&format!("{}.beats", fixture.name()));

        assert_eq!(audio.len(), fixture.wav_bytes().len(), "{}", fixture.name());
        assert!(!annotation.is_empty(), "{}", fixture.name());
    }
}

#[test]
fn the_committed_annotations_match_their_generator() {
    for fixture in synth::set() {
        let committed = read(&format!("{}.beats", fixture.name()));

        assert_eq!(
            String::from_utf8_lossy(&committed),
            fixture.annotation_text(),
            "{}",
            fixture.name()
        );
    }
}

#[test]
fn the_committed_audio_matches_its_generator() {
    for fixture in synth::set() {
        let committed = samples(&read(&format!("{}.wav", fixture.name())));

        assert_eq!(
            committed.len(),
            fixture.samples().len(),
            "{}",
            fixture.name()
        );
        let furthest = committed
            .iter()
            .zip(fixture.samples())
            .map(|(a, b)| i32::from(*a) - i32::from(*b))
            .map(i32::abs)
            .max()
            .unwrap_or(0);

        assert!(furthest <= 2, "{} drifted by {furthest}", fixture.name());
    }
}

#[test]
fn the_committed_set_is_under_its_size_ceiling() {
    let total: u64 = fs::read_dir(directory())
        .expect("the fixture directory is checked in")
        .map(|entry| entry.expect("the entry is readable"))
        .map(|entry| entry.metadata().expect("the metadata is readable").len())
        .sum();

    assert!(total <= 512 * 1024, "the set totals {total} bytes");
}

#[test]
fn nothing_is_checked_in_that_the_generator_did_not_write() {
    let expected: Vec<String> = synth::set()
        .iter()
        .flat_map(|fixture| {
            [
                format!("{}.wav", fixture.name()),
                format!("{}.beats", fixture.name()),
            ]
        })
        .chain(["README.md".to_owned()])
        .collect();

    for entry in fs::read_dir(directory()).expect("the fixture directory is checked in") {
        let name = entry.expect("the entry is readable").file_name();
        let name = name.to_string_lossy().into_owned();

        assert!(expected.contains(&name), "{name} is not part of the set");
    }
}
