//! The pass that reads a finished take, and what it puts on the screen.
//!
//! Nothing here is new analysis: the envelope, the beat tracker and the marks
//! are each tested on their own. What is stated here is that they run in that
//! order over a take a player closed, that what the player said about the take
//! reaches the tracker as priors, and that a beat found lands at the frame it
//! was played at.
//!
//! The other half is the thread. A take crosses off the callback and is
//! analysed somewhere else, so the facts are that a finished take reaches that
//! worker without the publisher waiting on it, and that one published while an
//! earlier one is still being analysed is analysed too.

use std::time::Duration;

use motif::device::AudioProfile;
use motif::fixtures::synth;
use motif::fixtures::{Drift, Recipe, Texture};
use motif::looper::{
    FinishedTake, LoopBuffer, LoopMarks, LoopWaveform, MarksReader, TakeWriter, analyse, analysing,
    marks_handoff, take_handoff,
};
use motif::seq::Bars;

const DOWNBEAT: char = '┃';
const BEAT: char = '│';
const CHORD_CHANGE: char = '◆';

/// A rate coarse enough to keep a fixture small, and a whole number of frames
/// to the envelope's hop.
const RATE: u32 = 8_000;

/// The block the profile states, which is what a callback hands the crossing.
const BLOCK: usize = 256;

/// How long the take is, well short of the buffer it is recorded into, so a
/// prior taken from the buffer rather than from the take grids differently.
const TAKE_FRAMES: usize = 4 * RATE as usize;

/// Frames between one struck beat and the next: half a second, which is 120
/// BPM, so [`TAKE_FRAMES`] holds eight beats.
const BEAT_FRAMES: usize = RATE as usize / 2;

const BEATS: usize = TAKE_FRAMES / BEAT_FRAMES;

/// Frames a strike sounds for, which is longer than one hop of the envelope
/// and far shorter than one beat.
const STRIKE_FRAMES: usize = 100;

const STRUCK: f32 = 0.2;
const ACCENTED: f32 = 0.8;

/// Beats to a bar in the fixture, which is what the accents mark out.
const BAR_BEATS: usize = 4;

/// More columns than the take has beats, so no two of them share one.
const COLUMNS: usize = 64;

/// More advances than a take of any length here takes to cross, so a crossing
/// that never ends fails rather than spinning.
const ADVANCES_ALLOWED: usize = 512;

/// How long a wait on the worker gives up after, and how often it looks.
///
/// Bounded, because an analysis that never arrives should fail the test rather
/// than hang the suite.
const LOOKS: usize = 200;
const PAUSE: Duration = Duration::from_millis(25);

fn profile() -> AudioProfile {
    AudioProfile {
        sample_rate: RATE,
        block_size: BLOCK as u32,
        max_loop_seconds: 6,
    }
}

/// A take of [`BEATS`] struck beats, accented at the start of every bar.
///
/// The accents are what a downbeat is found from: a grid of evenly struck
/// beats leaves every phase of the bar as good as every other.
fn struck() -> Vec<f32> {
    (0..TAKE_FRAMES)
        .map(|frame| match (frame % BEAT_FRAMES, frame / BEAT_FRAMES) {
            (0..STRIKE_FRAMES, beat) if beat.is_multiple_of(BAR_BEATS) => ACCENTED,
            (0..STRIKE_FRAMES, _) => STRUCK,
            _ => 0.0,
        })
        .collect()
}

/// Bars of one chord each, struck on every beat, at the rate the analyst times
/// against.
///
/// Rendered rather than written out here: a chord that a fold can hear is a
/// voicing, and `synth` already places one and knows what it is called.
fn voiced(bars: usize) -> Vec<f32> {
    let recipe = Recipe {
        tempo: 120.0,
        meter: BAR_BEATS,
        bars,
        drift: Drift::Steady,
        texture: Texture::Chords,
    };

    synth::rendered("harmony", recipe)
        .samples()
        .iter()
        .map(|sample| f32::from(*sample) / f32::from(i8::MAX))
        .collect()
}

fn recorded(samples: &[f32]) -> LoopBuffer {
    let mut buffer = LoopBuffer::for_profile(profile());
    buffer.record(samples);

    buffer
}

/// Hand `buffer`'s loop across as a callback does, counted as `bars`.
fn cross(writer: &mut TakeWriter, buffer: &LoopBuffer, bars: Option<Bars>) {
    writer.begin(buffer, bars);

    for _ in 0..ADVANCES_ALLOWED {
        if !writer.advance(buffer, BLOCK) {
            return;
        }
    }

    panic!("the take never finished crossing");
}

/// The marks a take of `samples`, counted as `bars`, is analysed into.
fn analysed(samples: &[f32], bars: Option<Bars>) -> LoopMarks {
    let (mut writer, mut reader) = take_handoff(profile());
    let buffer = recorded(samples);
    cross(&mut writer, &buffer, bars);

    let take: FinishedTake<'_> = reader.claim().expect("the take crossed");

    analyse(&take, RATE)
}

/// The columns of the drawn marks carrying any of `wanted`.
fn columns_holding(marks: &LoopMarks, shape: &LoopWaveform, wanted: &[char]) -> Vec<usize> {
    marks
        .drawn(shape, COLUMNS)
        .iter()
        .flat_map(|row| row.chars().enumerate().collect::<Vec<_>>())
        .filter(|(_at, glyph)| wanted.contains(glyph))
        .map(|(at, _glyph)| at)
        .collect()
}

/// The columns `frames` are drawn in, which is where a mark found at each of
/// them belongs.
fn columns_of(frames: &[usize], shape: &LoopWaveform) -> Vec<usize> {
    frames
        .iter()
        .filter_map(|frame| shape.column_of(*frame, COLUMNS))
        .collect()
}

fn every_beat() -> Vec<usize> {
    (0..BEATS).map(|beat| beat * BEAT_FRAMES).collect()
}

fn every_bar(bar_beats: usize) -> Vec<usize> {
    (0..BEATS)
        .step_by(bar_beats)
        .map(|beat| beat * BEAT_FRAMES)
        .collect()
}

/// Wait for the worker to publish marks that `settled` accepts, and give up
/// after [`LOOKS`] looks rather than waiting for ever.
fn waited_for(marks: &MarksReader, settled: impl Fn(&LoopMarks) -> bool) -> LoopMarks {
    let mut newest = LoopMarks::none();

    for _ in 0..LOOKS {
        if let Some(published) = marks.read() {
            newest = published;
        }
        if settled(&newest) {
            return newest;
        }
        std::thread::sleep(PAUSE);
    }

    panic!("the worker never published the marks the take was played with");
}

#[test]
fn a_beat_is_marked_at_the_frame_it_was_played_at() {
    let played = struck();
    let buffer = recorded(&played);

    let marks = analysed(&played, None);

    assert_eq!(
        columns_holding(&marks, buffer.waveform(), &[BEAT, DOWNBEAT]),
        columns_of(&every_beat(), buffer.waveform())
    );
}

#[test]
fn a_downbeat_is_marked_where_the_accents_put_the_bar() {
    let played = struck();
    let buffer = recorded(&played);

    let marks = analysed(&played, None);

    assert_eq!(
        columns_holding(&marks, buffer.waveform(), &[DOWNBEAT]),
        columns_of(&every_bar(BAR_BEATS), buffer.waveform())
    );
}

/// The take is four seconds of eight struck beats, so its length alone grids it
/// at eight. Stated as sixteen, it is gridded at sixteen: the count the player
/// closed the loop with is what the tracker is given, not what the audio
/// suggests on its own.
#[test]
fn the_count_the_player_stated_decides_how_many_beats_are_marked() {
    let played = struck();
    let buffer = recorded(&played);
    let doubled = Bars::of(2, 2 * BAR_BEATS).expect("two bars of eight beats is a count");

    let marks = analysed(&played, Some(doubled));

    assert_eq!(
        columns_holding(&marks, buffer.waveform(), &[BEAT, DOWNBEAT]).len(),
        2 * BEATS
    );
}

/// The same eight beats, counted in bars of two rather than of four, put a
/// downbeat on every other beat rather than on every fourth.
#[test]
fn the_meter_the_player_stated_decides_where_the_downbeats_fall() {
    let played = struck();
    let buffer = recorded(&played);
    let halved = Bars::of(BEATS / 2, 2).expect("four bars of two beats is a count");

    let marks = analysed(&played, Some(halved));

    assert_eq!(
        columns_holding(&marks, buffer.waveform(), &[DOWNBEAT]),
        columns_of(&every_bar(2), buffer.waveform())
    );
}

#[test]
fn a_take_the_writer_finishes_is_analysed_and_published() {
    let (mut writer, takes) = take_handoff(profile());
    let (found, marks) = marks_handoff();
    let played = struck();
    let buffer = recorded(&played);
    analysing(takes, RATE, found);

    cross(&mut writer, &buffer, None);

    let published = waited_for(&marks, |marks| marks != &LoopMarks::none());
    assert_eq!(
        columns_holding(&published, buffer.waveform(), &[BEAT, DOWNBEAT]),
        columns_of(&every_beat(), buffer.waveform())
    );
}

/// The handoff holds a take being read while the ones behind it are published,
/// so a player who closes another loop mid-analysis is analysed too rather
/// than dropped or made to wait.
#[test]
fn a_take_published_behind_one_being_analysed_is_analysed_too() {
    let (mut writer, takes) = take_handoff(profile());
    let (found, marks) = marks_handoff();
    let played = struck();
    let buffer = recorded(&played);
    let doubled = Bars::of(2, 2 * BAR_BEATS).expect("two bars of eight beats is a count");
    analysing(takes, RATE, found);

    cross(&mut writer, &buffer, None);
    cross(&mut writer, &buffer, Some(doubled));

    let published = waited_for(&marks, |marks| {
        columns_holding(marks, buffer.waveform(), &[BEAT, DOWNBEAT]).len() == 2 * BEATS
    });
    assert_eq!(
        columns_holding(&published, buffer.waveform(), &[BEAT, DOWNBEAT]).len(),
        2 * BEATS
    );
}

const VOICED_BARS: usize = 2;

/// The fixture voices one chord to the bar, so what the harmony does is change
/// on the downbeat — and the two are drawn on separate rows of one axis, which
/// is what makes them comparable column by column.
#[test]
fn a_chord_change_is_marked_on_the_bar_the_harmony_changes_on() {
    let played = voiced(VOICED_BARS);
    let buffer = recorded(&played);
    let counted = Bars::of(VOICED_BARS, BAR_BEATS).expect("two bars of four beats is a count");

    let marks = analysed(&played, Some(counted));

    assert_eq!(
        columns_holding(&marks, buffer.waveform(), &[CHORD_CHANGE]),
        columns_holding(&marks, buffer.waveform(), &[DOWNBEAT])
    );
}

#[test]
fn a_chord_change_is_marked_once_a_bar_over_a_chord_a_bar() {
    let played = voiced(VOICED_BARS);
    let buffer = recorded(&played);
    let counted = Bars::of(VOICED_BARS, BAR_BEATS).expect("two bars of four beats is a count");

    let marks = analysed(&played, Some(counted));

    assert_eq!(
        columns_holding(&marks, buffer.waveform(), &[CHORD_CHANGE]).len(),
        VOICED_BARS
    );
}

#[test]
fn a_take_of_percussion_alone_marks_no_chord_change() {
    let played = struck();
    let buffer = recorded(&played);

    let marks = analysed(&played, None);

    assert!(
        columns_holding(&marks, buffer.waveform(), &[CHORD_CHANGE]).is_empty(),
        "{:?}",
        marks.drawn(buffer.waveform(), COLUMNS)
    );
}
