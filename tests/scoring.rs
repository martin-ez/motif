//! Scoring a candidate sequence of positions against ground truth: what counts
//! as a hit, and what the numbers say when something does not.

use motif::fixtures::{Note, Score};
use std::time::Duration;

fn at(millis: &[u64]) -> Vec<Duration> {
    millis.iter().copied().map(Duration::from_millis).collect()
}

fn note(pitch: u8, onset: u64, offset: u64) -> Note {
    Note {
        pitch,
        onset: Duration::from_millis(onset),
        offset: Duration::from_millis(offset),
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "{actual} is not {expected}"
    );
}

#[test]
fn a_candidate_that_matches_exactly_scores_one() {
    let beats = at(&[0, 500, 1_000, 1_500]);

    assert_close(Score::of(&beats, &beats).f1(), 1.0);
}

#[test]
fn a_candidate_that_matches_nothing_scores_zero() {
    let score = Score::of(&at(&[0, 500, 1_000]), &at(&[200, 700, 1_200]));

    assert_close(score.f1(), 0.0);
}

#[test]
fn a_candidate_inside_the_tolerance_is_a_hit() {
    let score = Score::of(&at(&[1_000]), &at(&[1_069]));

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_candidate_on_the_tolerance_is_a_hit() {
    let score = Score::of(&at(&[1_000]), &at(&[1_070]));

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_candidate_beyond_the_tolerance_is_a_miss() {
    let score = Score::of(&at(&[1_000]), &at(&[1_071]));

    assert_eq!(score.hits(), 0);
}

#[test]
fn the_tolerance_reaches_as_far_early_as_late() {
    let score = Score::of(&at(&[1_000]), &at(&[930]));

    assert_eq!(score.hits(), 1);
}

#[test]
fn the_tolerance_is_seventy_milliseconds() {
    assert_eq!(Score::TOLERANCE, Duration::from_millis(70));
}

#[test]
fn two_candidates_in_one_window_are_one_hit() {
    let score = Score::of(&at(&[1_000]), &at(&[970, 1_030]));

    assert_eq!(score.hits(), 1);
    assert_close(score.precision(), 0.5);
    assert_close(score.recall(), 1.0);
}

#[test]
fn a_candidate_matched_once_cannot_match_again() {
    let score = Score::of(&at(&[1_000, 1_040]), &at(&[1_020]));

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_detector_running_at_double_rate_is_not_perfect() {
    let annotated = at(&[0, 500, 1_000, 1_500]);
    let detected = at(&[0, 250, 500, 750, 1_000, 1_250, 1_500, 1_750]);

    let score = Score::of(&annotated, &detected);

    assert_eq!(score.hits(), 4);
    assert_close(score.recall(), 1.0);
    assert_close(score.precision(), 0.5);
}

#[test]
fn an_annotation_takes_the_nearest_candidate_in_its_window() {
    let score = Score::of(&at(&[50, 120]), &at(&[0, 60]));

    assert_eq!(score.hits(), 1);
}

#[test]
fn precision_is_the_share_of_candidates_that_hit() {
    let score = Score::of(&at(&[0, 500]), &at(&[0, 500, 900, 1_400]));

    assert_close(score.precision(), 0.5);
}

#[test]
fn recall_is_the_share_of_annotations_that_were_found() {
    let score = Score::of(&at(&[0, 500, 1_000, 1_500]), &at(&[0, 1_000, 1_500]));

    assert_close(score.recall(), 0.75);
}

#[test]
fn f1_is_the_harmonic_mean_of_precision_and_recall() {
    let score = Score::of(&at(&[0, 500, 1_000, 1_500]), &at(&[0, 500, 5_000]));

    assert_close(score.precision(), 2.0 / 3.0);
    assert_close(score.recall(), 0.5);
    assert_close(score.f1(), 4.0 / 7.0);
}

#[test]
fn f1_falls_between_precision_and_recall() {
    let score = Score::of(&at(&[0, 500, 1_000, 1_500]), &at(&[0, 500]));

    assert!(score.recall() < score.f1() && score.f1() < score.precision());
}

#[test]
fn a_score_reports_what_it_counted() {
    let score = Score::of(&at(&[0, 500, 1_000]), &at(&[0, 500]));

    assert_eq!(
        (score.hits(), score.annotated(), score.detected()),
        (2, 3, 2)
    );
}

#[test]
fn a_candidate_with_nothing_in_it_scores_zero() {
    let score = Score::of(&at(&[0, 500]), &[]);

    assert_close(score.precision(), 0.0);
    assert_close(score.recall(), 0.0);
    assert_close(score.f1(), 0.0);
}

#[test]
fn an_annotation_with_nothing_in_it_scores_zero() {
    let score = Score::of(&[], &at(&[0, 500]));

    assert_close(score.precision(), 0.0);
    assert_close(score.recall(), 0.0);
    assert_close(score.f1(), 0.0);
}

#[test]
fn two_empty_sequences_score_zero() {
    let score = Score::of(&[], &[]);

    assert_close(score.f1(), 0.0);
}

#[test]
fn a_score_describes_what_it_counted() {
    let shown = Score::of(&at(&[0, 500, 1_000]), &at(&[0, 500])).to_string();

    assert!(shown.contains("F1"), "{shown}");
    assert!(shown.contains("hits 2/3"), "{shown}");
}

#[test]
fn the_note_tolerance_is_fifty_milliseconds() {
    assert_eq!(Score::NOTE_TOLERANCE, Duration::from_millis(50));
}

#[test]
fn a_transcription_that_matches_exactly_scores_one() {
    let notes = [note(60, 0, 400), note(64, 500, 900)];

    assert_close(Score::of_notes(&notes, &notes).f1(), 1.0);
}

#[test]
fn a_note_heard_at_the_wrong_pitch_is_a_miss() {
    let score = Score::of_notes(&[note(60, 0, 400)], &[note(61, 0, 400)]);

    assert_eq!(score.hits(), 0);
}

#[test]
fn a_note_heard_within_the_onset_tolerance_is_a_hit() {
    let score = Score::of_notes(&[note(60, 1_000, 1_500)], &[note(60, 1_050, 1_500)]);

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_note_heard_beyond_the_onset_tolerance_is_a_miss() {
    let score = Score::of_notes(&[note(60, 1_000, 1_500)], &[note(60, 1_051, 1_500)]);

    assert_eq!(score.hits(), 0);
}

#[test]
fn a_short_note_is_allowed_the_tolerance_at_its_end() {
    let score = Score::of_notes(&[note(60, 0, 200)], &[note(60, 0, 250)]);

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_short_note_released_beyond_the_tolerance_is_a_miss() {
    let score = Score::of_notes(&[note(60, 0, 200)], &[note(60, 0, 251)]);

    assert_eq!(score.hits(), 0);
}

#[test]
fn a_long_note_is_allowed_a_fifth_of_itself_at_its_end() {
    let score = Score::of_notes(&[note(60, 0, 1_000)], &[note(60, 0, 1_200)]);

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_long_note_released_beyond_a_fifth_of_itself_is_a_miss() {
    let score = Score::of_notes(&[note(60, 0, 1_000)], &[note(60, 0, 1_201)]);

    assert_eq!(score.hits(), 0);
}

#[test]
fn two_candidates_for_one_note_are_one_hit() {
    let score = Score::of_notes(
        &[note(60, 1_000, 1_500)],
        &[note(60, 980, 1_480), note(60, 1_020, 1_520)],
    );

    assert_eq!(score.hits(), 1);
    assert_close(score.precision(), 0.5);
    assert_close(score.recall(), 1.0);
}

#[test]
fn a_note_takes_the_nearest_candidate_that_is_still_free() {
    let score = Score::of_notes(
        &[note(60, 1_000, 1_500), note(60, 1_040, 1_540)],
        &[note(60, 1_020, 1_520)],
    );

    assert_eq!(score.hits(), 1);
}

#[test]
fn a_transcription_of_nothing_scores_zero() {
    let score = Score::of_notes(&[note(60, 0, 400)], &[]);

    assert_close(score.f1(), 0.0);
    assert_eq!(score.annotated(), 1);
}

#[test]
fn a_transcription_of_notes_nobody_played_scores_zero() {
    let score = Score::of_notes(&[], &[note(60, 0, 400)]);

    assert_close(score.f1(), 0.0);
    assert_eq!(score.detected(), 1);
}
