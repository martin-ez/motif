//! The ground-truth annotation format: what a well-formed file reads into,
//! and which line an unparseable one blames.

use motif::fixtures::{Annotation, AnnotationError, Beat};
use std::time::Duration;

const FOUR_FOUR: &str = "\
# two bars at 120 BPM
0.0 downbeat
0.5 beat
1.0 beat
1.5 beat
2.0 downbeat
";

fn parsed(text: &str) -> Annotation {
    text.parse().expect("the annotation is well formed")
}

fn rejected(text: &str) -> AnnotationError {
    text.parse::<Annotation>()
        .expect_err("the annotation is not well formed")
}

#[test]
fn an_annotation_reads_every_beat_in_order() {
    let annotation = parsed(FOUR_FOUR);

    let times: Vec<_> = annotation.beats().iter().map(|beat| beat.at).collect();

    assert_eq!(
        times,
        [
            Duration::ZERO,
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_millis(1_500),
            Duration::from_secs(2),
        ]
    );
}

#[test]
fn a_downbeat_is_also_a_beat() {
    let annotation = parsed(FOUR_FOUR);

    assert_eq!(annotation.beats().len(), 5);
}

#[test]
fn downbeats_are_the_beats_that_begin_a_bar() {
    let annotation = parsed(FOUR_FOUR);

    assert_eq!(
        annotation.downbeats().collect::<Vec<_>>(),
        [Duration::ZERO, Duration::from_secs(2)]
    );
}

#[test]
fn a_beat_says_whether_it_begins_a_bar() {
    let annotation = parsed(FOUR_FOUR);

    assert_eq!(
        annotation.beats()[1],
        Beat {
            at: Duration::from_millis(500),
            is_downbeat: false,
        }
    );
}

#[test]
fn a_timestamp_keeps_its_millisecond_precision() {
    let annotation = parsed("0.001 downbeat\n0.070 beat\n");

    assert_eq!(
        annotation.beats()[1].at - annotation.beats()[0].at,
        Duration::from_millis(69)
    );
}

#[test]
fn fields_may_be_separated_by_any_run_of_whitespace() {
    let annotation = parsed("0.0\t\tdownbeat\n   1.0    beat\n");

    assert_eq!(annotation.beats().len(), 2);
}

#[test]
fn blank_lines_are_ignored() {
    let annotation = parsed("\n0.0 downbeat\n\n   \n1.0 beat\n");

    assert_eq!(annotation.beats().len(), 2);
}

#[test]
fn comment_lines_are_ignored() {
    let annotation = parsed("# a fixture\n0.0 downbeat\n  # indented\n1.0 beat\n");

    assert_eq!(annotation.beats().len(), 2);
}

#[test]
fn a_line_that_is_not_a_timestamp_and_a_kind_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat\n0.5\n"),
        AnnotationError::Malformed { line: 2 }
    );
}

#[test]
fn a_line_with_a_trailing_field_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat # the first bar\n"),
        AnnotationError::Malformed { line: 1 }
    );
}

#[test]
fn a_timestamp_that_is_not_a_number_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat\nhalfway beat\n"),
        AnnotationError::Timestamp { line: 2 }
    );
}

#[test]
fn a_negative_timestamp_is_an_error() {
    assert_eq!(
        rejected("-0.5 downbeat\n"),
        AnnotationError::Timestamp { line: 1 }
    );
}

#[test]
fn an_unknown_beat_kind_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat\n0.5 offbeat\n"),
        AnnotationError::BeatKind { line: 2 }
    );
}

#[test]
fn a_timestamp_that_goes_backwards_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat\n1.0 beat\n0.5 beat\n"),
        AnnotationError::OutOfOrder { line: 3 }
    );
}

#[test]
fn a_repeated_timestamp_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat\n1.0 beat\n1.0 beat\n"),
        AnnotationError::OutOfOrder { line: 3 }
    );
}

#[test]
fn an_annotation_with_no_beats_is_an_error() {
    assert_eq!(rejected("# nothing here\n\n"), AnnotationError::Empty);
}

#[test]
fn the_blamed_line_counts_comments_and_blank_lines() {
    assert_eq!(
        rejected("# a fixture\n\n0.0 downbeat\n0.5 wrong\n"),
        AnnotationError::BeatKind { line: 4 }
    );
}

#[test]
fn an_error_on_a_line_reports_which_line() {
    assert_eq!(AnnotationError::OutOfOrder { line: 7 }.line(), Some(7));
}

#[test]
fn an_error_with_no_line_reports_none() {
    assert_eq!(AnnotationError::Empty.line(), None);
}

#[test]
fn an_error_describes_itself_and_names_its_line() {
    assert_eq!(
        AnnotationError::BeatKind { line: 4 }.to_string(),
        "line 4: the beat kind is neither 'beat' nor 'downbeat'"
    );
}

#[test]
fn an_empty_annotation_describes_itself() {
    assert_eq!(
        AnnotationError::Empty.to_string(),
        "the annotation has no beats"
    );
}
