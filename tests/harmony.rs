//! What harmony a take holds, heard over the beats it was played on.
//!
//! Two halves. [`Chroma`] is the fold: a window's magnitudes become twelve
//! weights, and the chord those weights sit nearest. [`chords`] is the pass
//! over a whole take: one chroma to the beat, and a label that runs for as
//! long as it holds rather than being restated on every beat under it.
//!
//! The pass is crude on purpose, so what is stated here is what it promises
//! rather than how well it does it. What it scores is the harness's to say.

use motif::analysis::{Chroma, Priors, Transform, chords, track};
use motif::fixtures::harness;
use motif::fixtures::synth::{self, Fixture};
use motif::fixtures::{Chord, ChordLabel, Comparison};
use std::f64::consts::TAU;
use std::time::Duration;

/// The rate the synthetic fixtures are rendered at, coarse enough to keep a
/// take here small and fine enough to resolve a semitone.
const RATE: u32 = 8_000;

/// Half a second to the beat, which is 120 BPM.
const BEAT: Duration = Duration::from_millis(500);

/// A window long enough to resolve a semitone at this rate, which is what the
/// pass plans its own transform for.
const WINDOW: usize = 2_048;

const CONCERT_A: f64 = 440.0;
const CONCERT_A_PITCH: f64 = 69.0;
const SEMITONES: f64 = 12.0;
const MIDDLE_C: u8 = 60;
const LEVEL: f32 = 0.3;

const MAJOR: [u8; 3] = [0, 4, 7];
const MINOR: [u8; 3] = [0, 3, 7];
const DOMINANT: [u8; 4] = [0, 4, 7, 10];

fn hertz(pitch: u8) -> f64 {
    CONCERT_A * ((f64::from(pitch) - CONCERT_A_PITCH) / SEMITONES).exp2()
}

fn frames(span: Duration) -> usize {
    (span.as_secs_f64() * f64::from(RATE)) as usize
}

/// `frames` of the pitches in `voicing` sounding together, each a sine.
fn voiced(voicing: &[u8], frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|frame| {
            let elapsed = frame as f64 / f64::from(RATE);
            voicing
                .iter()
                .map(|pitch| (TAU * hertz(*pitch) * elapsed).sin())
                .sum::<f64>() as f32
                * LEVEL
        })
        .collect()
}

/// The chord `root` and `intervals` spell, voiced upwards from middle C.
fn chord_of(root: u8, intervals: &[u8]) -> Vec<u8> {
    intervals
        .iter()
        .map(|interval| MIDDLE_C + root + interval)
        .collect()
}

/// The chroma of one window of `voicing`.
fn chroma_of(voicing: &[u8]) -> Chroma {
    let transform = Transform::of(WINDOW).expect("a power of two");
    let magnitudes = transform
        .magnitudes(&voiced(voicing, WINDOW))
        .expect("a full window")
        .to_vec();

    Chroma::of(&magnitudes, RATE)
}

/// A take of one voicing to the beat, in the order they are given.
fn played(progression: &[Vec<u8>]) -> (Vec<f32>, Vec<Duration>) {
    let mut samples = Vec::new();
    let mut beats = Vec::new();

    for (index, voicing) in progression.iter().enumerate() {
        beats.push(BEAT * index as u32);
        samples.extend(voiced(voicing, frames(BEAT)));
    }

    (samples, beats)
}

fn labels(heard: &[Chord]) -> Vec<String> {
    heard.iter().map(|chord| chord.label.to_string()).collect()
}

#[test]
fn a_tone_an_octave_up_is_folded_into_the_class_under_it() {
    let voiced_over: Vec<u8> = chord_of(0, &MAJOR)
        .iter()
        .map(|pitch| pitch + if *pitch % 12 == 4 { 12 } else { 0 })
        .collect();

    assert_eq!(chroma_of(&voiced_over).nearest().to_string(), "C:maj");
}

#[test]
fn a_chroma_of_silence_is_no_chord() {
    assert_eq!(chroma_of(&[]).nearest(), ChordLabel::Silent);
}

#[test]
fn a_pitch_under_the_range_the_fold_counts_is_no_chord() {
    let rumble = chroma_of(&[MIDDLE_C - 24]);

    assert_eq!(rumble.nearest(), ChordLabel::Silent);
}

#[test]
fn every_class_sounding_at_once_is_no_chord() {
    let chromatic: Vec<u8> = (0..12).map(|semitone| MIDDLE_C + semitone).collect();

    assert_eq!(chroma_of(&chromatic).nearest(), ChordLabel::Silent);
}

#[test]
fn a_triad_is_nearest_the_chord_it_voices() {
    let chroma = chroma_of(&chord_of(0, &MAJOR));

    assert_eq!(chroma.nearest().to_string(), "C:maj");
}

#[test]
fn a_minor_third_is_not_heard_as_a_major_one() {
    let chroma = chroma_of(&chord_of(0, &MINOR));

    assert_eq!(chroma.nearest().to_string(), "C:min");
}

#[test]
fn a_root_is_heard_wherever_the_chord_is_rooted() {
    let chroma = chroma_of(&chord_of(5, &MAJOR));

    assert_eq!(chroma.nearest().to_string(), "F:maj");
}

#[test]
fn a_seventh_is_heard_over_the_triad_it_stacks_on() {
    let chroma = chroma_of(&chord_of(7, &DOMINANT));

    assert_eq!(chroma.nearest().to_string(), "G:7");
}

#[test]
fn a_beat_is_labelled_with_the_chord_sounding_over_it() {
    let (samples, beats) = played(&[chord_of(0, &MAJOR)]);

    assert_eq!(labels(&chords(&samples, RATE, &beats)), ["C:maj"]);
}

#[test]
fn a_span_starts_at_the_beat_its_chord_was_first_heard_on() {
    let (samples, beats) = played(&[chord_of(0, &MAJOR), chord_of(5, &MAJOR)]);

    let heard = chords(&samples, RATE, &beats);

    assert_eq!(labels(&heard), ["C:maj", "F:maj"]);
    assert_eq!(heard[1].from, BEAT);
}

#[test]
fn a_chord_held_over_two_beats_is_one_span() {
    let (samples, beats) = played(&[chord_of(0, &MAJOR), chord_of(0, &MAJOR)]);

    let heard = chords(&samples, RATE, &beats);

    assert_eq!(labels(&heard), ["C:maj"]);
    assert_eq!(heard[0].from, Duration::ZERO);
}

#[test]
fn a_span_ends_where_the_one_after_it_starts() {
    let (samples, beats) = played(&[chord_of(0, &MAJOR), chord_of(5, &MAJOR)]);

    let heard = chords(&samples, RATE, &beats);

    assert_eq!(heard[0].to, heard[1].from);
}

#[test]
fn the_last_span_runs_to_the_end_of_the_take() {
    let (samples, beats) = played(&[chord_of(0, &MAJOR), chord_of(5, &MAJOR)]);

    let heard = chords(&samples, RATE, &beats);

    assert_eq!(heard[heard.len() - 1].to, BEAT * 2);
}

#[test]
fn silence_between_two_chords_is_a_span_of_its_own() {
    let mut samples = Vec::new();
    samples.extend(voiced(&chord_of(0, &MAJOR), frames(BEAT)));
    samples.extend(voiced(&[], frames(BEAT)));
    samples.extend(voiced(&chord_of(5, &MAJOR), frames(BEAT)));
    let beats = [Duration::ZERO, BEAT, BEAT * 2];

    let heard = chords(&samples, RATE, &beats);

    assert_eq!(labels(&heard), ["C:maj", "N", "F:maj"]);
}

#[test]
fn a_take_with_no_beats_holds_no_harmony() {
    let (samples, _beats) = played(&[chord_of(0, &MAJOR)]);

    assert!(chords(&samples, RATE, &[]).is_empty());
}

#[test]
fn a_beat_past_the_end_of_the_take_hears_nothing_over_it() {
    let (samples, _beats) = played(&[chord_of(0, &MAJOR)]);

    assert_eq!(labels(&chords(&samples, RATE, &[BEAT * 4])), ["N"]);
}

#[test]
fn a_chord_is_heard_the_same_whichever_octave_it_is_voiced_in() {
    let low = chord_of(0, &MAJOR);
    let high: Vec<u8> = low.iter().map(|pitch| pitch + 12).collect();

    let (under, beats) = played(&[low]);
    let (over, _) = played(&[high]);

    assert_eq!(
        labels(&chords(&under, RATE, &beats)),
        labels(&chords(&over, RATE, &beats))
    );
}

#[test]
fn the_window_the_pass_plans_resolves_a_semitone_at_this_rate() {
    assert_eq!(Chroma::window(RATE), WINDOW);
}

/// How many drawn fixtures the standing figure is taken over: enough that one
/// misheard bar does not decide it, and few enough to stay a test.
const DRAWN: usize = 4;

/// The floor the pass has to clear over the set drawn for development.
///
/// Well under what it reaches, because what this pins is that the pass answers
/// at all and goes on answering. The figure itself is `score-chords`' to
/// report, over the evaluation seed rather than this one.
const CLEARS: f64 = 0.5;

/// What the looper tells the tracker: how long the take ran, and the count it
/// was played in.
fn told(fixture: &Fixture) -> Priors {
    let beats = fixture.beats();
    let last = beats.last().expect("a drawn fixture has beats").at;

    Priors::of_take(last + last / (beats.len() as u32 - 1))
        .with_meter(fixture.recipe().meter)
        .with_bars(fixture.recipe().bars)
}

fn as_played(fixture: &Fixture) -> Vec<f32> {
    fixture
        .samples()
        .iter()
        .map(|sample| f32::from(*sample) / f32::from(i8::MAX))
        .collect()
}

#[test]
fn the_pass_carries_a_figure_over_the_drawn_harmonic_set() {
    let set = synth::drawn_chords(synth::DEVELOPMENT[0], DRAWN);

    let report = harness::measure_rendered_chords(&set, Comparison::Root, |fixture| {
        let played = as_played(fixture);
        let grid = track(played.iter().copied(), synth::SAMPLE_RATE, told(fixture));

        chords(&played, synth::SAMPLE_RATE, grid.beats())
    });

    assert_eq!(report.rows().len(), DRAWN);
    assert!(report.mean() > CLEARS, "{report}");
}
