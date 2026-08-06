//! The synthetic fixture set: what it covers, what it renders, and that the
//! files checked in are still the ones its generator produces.

use motif::fixtures::synth::{self, Fixture, SAMPLE_RATE, Voice};
use motif::fixtures::{Annotation, Beat, ChordLabel};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const CEILING: u64 = 640 * 1024;

const HARMONY: &str = "chords-150-4-4";
const LINE: &str = "line-150-4-4";

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
    if counted > 0 {
        lengths.push(counted);
    }
    lengths
}

fn plays_percussion(fixture: &Fixture) -> bool {
    fixture
        .onsets()
        .iter()
        .any(|onset| matches!(onset.voice, Voice::Accent | Voice::Tick))
}

fn frames(at: Duration) -> usize {
    (at.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as usize
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
        assert_eq!(annotation.chords(), fixture.chords(), "{}", fixture.name());
        assert_eq!(annotation.notes(), fixture.notes(), "{}", fixture.name());
    }
}

#[test]
fn an_annotation_is_headed_by_the_fixture_it_describes() {
    for fixture in synth::set() {
        let heading = fixture.annotation_text().lines().next().unwrap().to_owned();

        assert_eq!(
            heading,
            format!("# {}: {}", fixture.name(), fixture.description())
        );
    }
}

#[test]
fn every_fixture_is_short_enough_to_belong_to_the_set() {
    for fixture in synth::set() {
        let length =
            Duration::from_secs_f64(fixture.samples().len() as f64 / f64::from(SAMPLE_RATE));

        assert!(
            length <= Duration::from_secs(10),
            "{} runs {length:?}",
            fixture.name()
        );
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
    let metres: Vec<_> = synth::set().iter().flat_map(beats_per_bar).collect();

    assert!(metres.contains(&3), "bar lengths: {metres:?}");
    assert!(metres.contains(&4), "bar lengths: {metres:?}");
}

#[test]
fn a_waltz_is_three_beats_to_the_bar_throughout() {
    assert_eq!(beats_per_bar(&named("waltz-150-3-4")), [3, 3, 3, 3]);
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
fn a_tempo_ramp_spans_the_tempi_it_is_named_for() {
    let intervals = intervals(&named("ramp-100-140-4-4"));
    let tempo = |interval: Duration| 60.0 / interval.as_secs_f64();

    assert!((tempo(intervals[0]) - 100.0).abs() < 0.1, "{intervals:?}");
    assert!(
        (tempo(intervals[intervals.len() - 1]) - 140.0).abs() < 0.1,
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
fn a_syncopated_onset_falls_midway_between_the_beats_it_sits_between() {
    let fixture = named("syncopated-120-4-4");
    let beats = fixture.beats();

    for onset in fixture.onsets() {
        let Some(before) = beats.iter().rev().find(|beat| beat.at < onset.at) else {
            continue;
        };
        let Some(after) = beats.iter().find(|beat| beat.at > onset.at) else {
            continue;
        };
        let early = onset.at - before.at;
        let late = after.at - onset.at;

        assert!(
            early.abs_diff(late) <= Duration::from_nanos(u64::from(1_000_000_000 / SAMPLE_RATE)),
            "an onset at {:?} sat {early:?} after one beat and {late:?} before the next",
            onset.at
        );
    }
}

#[test]
fn every_downbeat_is_sounded() {
    for fixture in synth::set() {
        for beat in fixture.beats().iter().filter(|beat| beat.is_downbeat) {
            assert!(
                fixture.onsets().iter().any(|onset| onset.at == beat.at),
                "{} at {:?}",
                fixture.name(),
                beat.at
            );
        }
    }
}

#[test]
fn every_percussive_downbeat_is_accented() {
    for fixture in synth::set().iter().filter(|f| plays_percussion(f)) {
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
fn the_committed_wav_headers_match_their_generator() {
    for fixture in synth::set() {
        let committed = read(&format!("{}.wav", fixture.name()));

        assert_eq!(
            committed[..44],
            fixture.wav_bytes()[..44],
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

    assert!(total <= CEILING, "the set totals {total} bytes");
}

#[test]
fn the_set_carries_a_fixture_of_annotated_harmony() {
    assert!(!named(HARMONY).chords().is_empty());
}

#[test]
fn the_set_carries_a_fixture_of_a_monophonic_line() {
    assert!(!named(LINE).notes().is_empty());
}

#[test]
fn only_the_harmony_fixture_annotates_chords() {
    let annotated: Vec<_> = synth::set()
        .iter()
        .filter(|fixture| !fixture.chords().is_empty())
        .map(|fixture| fixture.name().to_owned())
        .collect();

    assert_eq!(annotated, [HARMONY]);
}

#[test]
fn only_the_line_fixture_annotates_notes() {
    let annotated: Vec<_> = synth::set()
        .iter()
        .filter(|fixture| !fixture.notes().is_empty())
        .map(|fixture| fixture.name().to_owned())
        .collect();

    assert_eq!(annotated, [LINE]);
}

#[test]
fn the_progression_is_one_chord_to_every_bar() {
    let fixture = named(HARMONY);

    assert_eq!(fixture.chords().len(), beats_per_bar(&fixture).len());
}

#[test]
fn every_chord_starts_on_a_downbeat() {
    let fixture = named(HARMONY);

    for chord in fixture.chords() {
        assert!(
            fixture
                .beats()
                .iter()
                .any(|beat| beat.is_downbeat && beat.at == chord.from),
            "a chord starts at {:?}",
            chord.from
        );
    }
}

#[test]
fn the_chords_run_end_to_end_with_no_gap_between_them() {
    let fixture = named(HARMONY);

    for pair in fixture.chords().windows(2) {
        assert_eq!(pair[0].to, pair[1].from);
    }
}

#[test]
fn the_progression_does_not_repeat_one_chord_throughout() {
    let fixture = named(HARMONY);
    let mut labels: Vec<_> = fixture
        .chords()
        .iter()
        .map(|chord| chord.label.to_string())
        .collect();
    labels.sort();
    labels.dedup();

    assert!(labels.len() > 1, "the progression is {labels:?}");
}

#[test]
fn a_chord_puts_its_root_in_the_audio() {
    let fixture = named(HARMONY);

    for chord in fixture.chords() {
        let ChordLabel::Sounding(root, _) = chord.label else {
            continue;
        };
        assert!(
            fixture.onsets().iter().any(|onset| onset.at == chord.from
                && matches!(onset.voice, Voice::Tone { pitch, .. }
                    if pitch % 12 == root.semitone())),
            "{} is not voiced at {:?}",
            chord.label,
            chord.from
        );
    }
}

#[test]
fn a_chord_is_struck_again_on_every_beat_of_its_bar() {
    let fixture = named(HARMONY);

    for chord in fixture.chords() {
        let under = fixture
            .beats()
            .iter()
            .filter(|beat| chord.from <= beat.at && beat.at < chord.to);
        for beat in under {
            assert!(
                fixture
                    .onsets()
                    .iter()
                    .any(|onset| onset.at == beat.at && matches!(onset.voice, Voice::Tone { .. })),
                "nothing is struck at {:?}",
                beat.at
            );
        }
    }
}

#[test]
fn a_struck_chord_releases_before_the_next_beat() {
    let fixture = named(HARMONY);
    let beats = fixture.beats();

    for onset in fixture.onsets() {
        let Voice::Tone { until, .. } = onset.voice else {
            continue;
        };
        let Some(next) = beats.iter().find(|beat| beat.at > onset.at) else {
            continue;
        };

        assert!(
            until < next.at,
            "a strike at {:?} runs to {until:?}",
            onset.at
        );
    }
}

#[test]
fn a_line_sounds_one_note_at_a_time() {
    let fixture = named(LINE);

    for pair in fixture.notes().windows(2) {
        assert!(pair[0].offset <= pair[1].onset, "{pair:?}");
    }
}

#[test]
fn a_line_does_not_hold_every_note_for_the_same_length() {
    let fixture = named(LINE);
    let mut lengths: Vec<_> = fixture
        .notes()
        .iter()
        .map(|note| note.offset - note.onset)
        .collect();
    lengths.sort();
    lengths.dedup();

    assert!(lengths.len() > 1, "the lengths are {lengths:?}");
}

#[test]
fn a_note_stops_before_the_next_one_starts_so_its_end_is_annotated() {
    let fixture = named(LINE);

    for pair in fixture.notes().windows(2) {
        assert!(pair[0].offset < pair[1].onset, "{pair:?}");
    }
}

#[test]
fn every_note_is_sounded_as_a_tone_of_its_pitch() {
    let fixture = named(LINE);

    for note in fixture.notes() {
        assert!(
            fixture.onsets().iter().any(|onset| onset.at == note.onset
                && onset.voice
                    == Voice::Tone {
                        pitch: note.pitch,
                        until: note.offset,
                    }),
            "{note:?} is not sounded"
        );
    }
}

#[test]
fn every_chord_and_note_lands_on_the_sample_grid() {
    let frame = u128::from(1_000_000_000 / SAMPLE_RATE);
    for fixture in synth::set() {
        for chord in fixture.chords() {
            assert_eq!(chord.from.as_nanos() % frame, 0, "{}", fixture.name());
            assert_eq!(chord.to.as_nanos() % frame, 0, "{}", fixture.name());
        }
        for note in fixture.notes() {
            assert_eq!(note.onset.as_nanos() % frame, 0, "{}", fixture.name());
            assert_eq!(note.offset.as_nanos() % frame, 0, "{}", fixture.name());
        }
    }
}

#[test]
fn a_note_is_rendered_at_the_pitch_it_annotates() {
    let fixture = named(LINE);
    let note = fixture.notes()[1];
    let hertz = 440.0 * 2f64.powf((f64::from(note.pitch) - 69.0) / 12.0);
    let from = frames(note.onset + Duration::from_millis(50));
    let window = SAMPLE_RATE as usize / 10;

    let crossings = fixture.samples()[from..from + window]
        .windows(2)
        .filter(|pair| pair[0].signum() != pair[1].signum())
        .count() as f64;
    let sounded = crossings / 2.0 * 10.0;

    assert!(
        (sounded - hertz).abs() < hertz * 0.15,
        "a note annotated at {hertz:.1} Hz sounded at about {sounded:.1} Hz"
    );
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
