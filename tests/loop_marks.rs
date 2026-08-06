//! What an analyser found, drawn over the loop it found it in.
//!
//! A mark is a frame, and the loop's own summary is what turns that into a
//! column, so the mapping is stated once here against a length whose arithmetic
//! a reader can follow and left to `loop_waveform.rs` everywhere else.
//!
//! The rest is what a cell can hold. Two marks falling in one column is the
//! ordinary case on a screen narrower than the loop is long, so the tests state
//! which of them is drawn, and that a chord change on a beat costs neither of
//! them its place.

use motif::device::DeviceProfile;
use motif::looper::{LoopMarks, LoopWaveform, Mark};
use motif::ui::columns_of;

const COLUMNS: usize = DeviceProfile::TARGET.screen.columns;

/// A loop of four frames a bucket, so the bucket a frame falls in is arithmetic
/// a reader can do.
const FRAMES_PER_BUCKET: u64 = 4;
const LONG: u64 = LoopWaveform::BUCKETS as u64 * FRAMES_PER_BUCKET;

const DOWNBEAT: char = '┃';
const BEAT: char = '│';
const CHORD_CHANGE: char = '◆';
const ONSET: char = '•';

/// Two frames of one bucket, which is one column however wide the region is.
const A_FRAME: u64 = 200;
const THE_SAME_COLUMN: u64 = A_FRAME + 1;

/// A frame far enough from [`A_FRAME`] to be drawn in a column of its own.
const A_LATER_FRAME: u64 = 400;

fn silent(frames: u64) -> LoopWaveform {
    let mut waveform = LoopWaveform::EMPTY;
    waveform.take(0, vec![0.0; frames as usize]);

    waveform
}

fn holding(marks: &[(u64, Mark)]) -> LoopMarks {
    let mut found = LoopMarks::none();
    for &(at, mark) in marks {
        found.add(at, mark);
    }

    found
}

fn over_the_loop(marks: &[(u64, Mark)]) -> [String; LoopMarks::ROWS] {
    holding(marks).drawn(&silent(LONG), COLUMNS)
}

fn grid(marks: &[(u64, Mark)]) -> String {
    over_the_loop(marks)[0].clone()
}

fn events(marks: &[(u64, Mark)]) -> String {
    over_the_loop(marks)[1].clone()
}

fn columns_holding(row: &str, glyph: char) -> Vec<usize> {
    row.chars()
        .enumerate()
        .filter(|(_at, drawn)| *drawn == glyph)
        .map(|(at, _drawn)| at)
        .collect()
}

/// Where the loop draws a frame of its own, so a test states the column it
/// expects rather than the arithmetic that finds it.
fn column_of(frame: u64) -> usize {
    silent(LONG)
        .column_of(frame as usize, COLUMNS)
        .expect("the frame is inside the loop")
}

#[test]
fn a_loop_nobody_has_analysed_draws_blank_rows() {
    assert!(over_the_loop(&[]).iter().all(|row| row.trim().is_empty()));
}

/// The alignment stated once in full. Frame 200 of this loop is in its
/// fiftieth bucket, and a region of the screen's width draws that bucket in its
/// twenty-sixth column — not the twenty-fifth, which is where a mark placed by
/// its share of the loop rather than by its bucket would land.
#[test]
fn a_beat_is_drawn_in_the_column_the_loop_draws_its_frame_in() {
    assert_eq!(columns_holding(&grid(&[(A_FRAME, Mark::Beat)]), BEAT), [26]);
}

#[test]
fn a_later_beat_is_drawn_further_right() {
    let drawn = grid(&[(A_FRAME, Mark::Beat), (A_LATER_FRAME, Mark::Beat)]);

    assert_eq!(
        columns_holding(&drawn, BEAT),
        [column_of(A_FRAME), column_of(A_LATER_FRAME)]
    );
}

#[test]
fn a_downbeat_is_drawn_heavier_than_a_beat() {
    let drawn = grid(&[(A_FRAME, Mark::Downbeat)]);

    assert_eq!(columns_holding(&drawn, DOWNBEAT), [column_of(A_FRAME)]);
    assert_eq!(columns_holding(&drawn, BEAT), []);
}

#[test]
fn a_chord_change_is_drawn_under_the_grid() {
    let marks = [(A_FRAME, Mark::ChordChange)];

    assert!(grid(&marks).trim().is_empty());
    assert_eq!(
        columns_holding(&events(&marks), CHORD_CHANGE),
        [column_of(A_FRAME)]
    );
}

#[test]
fn a_note_onset_is_drawn_under_the_grid() {
    let marks = [(A_FRAME, Mark::Onset)];

    assert!(grid(&marks).trim().is_empty());
    assert_eq!(
        columns_holding(&events(&marks), ONSET),
        [column_of(A_FRAME)]
    );
}

#[test]
fn a_downbeat_takes_the_column_from_a_beat_beside_it() {
    let drawn = grid(&[(A_FRAME, Mark::Beat), (THE_SAME_COLUMN, Mark::Downbeat)]);

    assert_eq!(columns_holding(&drawn, DOWNBEAT), [column_of(A_FRAME)]);
    assert_eq!(columns_holding(&drawn, BEAT), []);
}

#[test]
fn a_beat_found_after_a_downbeat_does_not_take_its_column() {
    let drawn = grid(&[(A_FRAME, Mark::Downbeat), (THE_SAME_COLUMN, Mark::Beat)]);

    assert_eq!(columns_holding(&drawn, DOWNBEAT), [column_of(A_FRAME)]);
    assert_eq!(columns_holding(&drawn, BEAT), []);
}

#[test]
fn a_chord_change_takes_the_column_from_a_note_onset() {
    let drawn = events(&[(A_FRAME, Mark::Onset), (THE_SAME_COLUMN, Mark::ChordChange)]);

    assert_eq!(columns_holding(&drawn, CHORD_CHANGE), [column_of(A_FRAME)]);
    assert_eq!(columns_holding(&drawn, ONSET), []);
}

#[test]
fn a_note_onset_found_after_a_chord_change_does_not_take_its_column() {
    let drawn = events(&[(A_FRAME, Mark::ChordChange), (THE_SAME_COLUMN, Mark::Onset)]);

    assert_eq!(columns_holding(&drawn, CHORD_CHANGE), [column_of(A_FRAME)]);
    assert_eq!(columns_holding(&drawn, ONSET), []);
}

/// The reason the marks take two rows rather than one: harmony changes on the
/// beat, and a row that held both would show one of them and drop the other.
#[test]
fn a_chord_change_on_a_beat_draws_both_of_them() {
    let marks = [(A_FRAME, Mark::Beat), (A_FRAME, Mark::ChordChange)];

    assert_eq!(columns_holding(&grid(&marks), BEAT), [column_of(A_FRAME)]);
    assert_eq!(
        columns_holding(&events(&marks), CHORD_CHANGE),
        [column_of(A_FRAME)]
    );
}

#[test]
fn a_mark_at_the_end_of_the_loop_is_dropped() {
    let drawn = over_the_loop(&[(LONG, Mark::Beat), (LONG + FRAMES_PER_BUCKET, Mark::Onset)]);

    assert!(drawn.iter().all(|row| row.trim().is_empty()));
}

#[test]
fn marks_over_a_loop_with_nothing_recorded_draw_nothing() {
    let drawn = holding(&[(A_FRAME, Mark::Beat)]).drawn(&LoopWaveform::EMPTY, COLUMNS);

    assert!(drawn.iter().all(|row| row.trim().is_empty()));
}

#[test]
fn every_row_is_as_wide_as_the_region_asked_for() {
    let drawn = over_the_loop(&[(A_FRAME, Mark::Downbeat), (A_LATER_FRAME, Mark::Onset)]);

    assert!(drawn.iter().all(|row| columns_of(row) == COLUMNS));
}

#[test]
fn a_region_with_no_columns_draws_empty_rows() {
    let drawn = holding(&[(A_FRAME, Mark::Beat)]).drawn(&silent(LONG), 0);

    assert!(drawn.iter().all(String::is_empty));
}
