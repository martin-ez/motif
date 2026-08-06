//! The ground-truth annotation format: what a well-formed file reads into,
//! and which line an unparseable one blames.

use motif::fixtures::{Annotation, AnnotationError, Beat, Chord, ChordLabel, Note};
use std::time::Duration;

const FOUR_FOUR: &str = "\
# two bars at 120 BPM
0.0 downbeat
0.5 beat
1.0 beat
1.5 beat
2.0 downbeat
";

const A_BAR_OF_EACH: &str = "\
0.0 downbeat
1.0 beat
0.0 chord C:maj
2.0 chord A:min
4.0 chord N
0.0 note 60 0.4
0.5 note 64 0.9
";

fn parsed(text: &str) -> Annotation {
    text.parse().expect("the annotation is well formed")
}

fn rejected(text: &str) -> AnnotationError {
    text.parse::<Annotation>()
        .expect_err("the annotation is not well formed")
}

fn label(text: &str) -> ChordLabel {
    ChordLabel::parse(text).unwrap_or_else(|| panic!("{text} is a chord label"))
}

fn beats_of(text: &str) -> String {
    format!("0.0 downbeat\n{text}")
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
fn an_unknown_entry_kind_is_an_error() {
    assert_eq!(
        rejected("0.0 downbeat\n0.5 offbeat\n"),
        AnnotationError::EntryKind { line: 2 }
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
fn a_negatively_signed_zero_is_an_error() {
    assert_eq!(
        rejected("-0.0 downbeat\n"),
        AnnotationError::Timestamp { line: 1 }
    );
}

#[test]
fn an_annotation_with_no_beats_is_an_error() {
    assert_eq!(rejected("# nothing here\n\n"), AnnotationError::Empty);
}

#[test]
fn an_annotation_with_no_downbeats_is_an_error() {
    assert_eq!(
        rejected("0.0 beat\n0.5 beat\n1.0 beat\n"),
        AnnotationError::NoDownbeats
    );
}

#[test]
fn an_annotation_need_not_begin_on_a_downbeat() {
    let annotation = parsed("0.0 beat\n0.5 beat\n1.0 downbeat\n");

    assert_eq!(
        annotation.downbeats().collect::<Vec<_>>(),
        [Duration::from_secs(1)]
    );
}

#[test]
fn an_annotation_with_no_downbeats_reports_no_line() {
    assert_eq!(AnnotationError::NoDownbeats.line(), None);
}

#[test]
fn an_annotation_with_no_downbeats_describes_itself() {
    assert_eq!(
        AnnotationError::NoDownbeats.to_string(),
        "the annotation has no downbeats"
    );
}

#[test]
fn the_blamed_line_counts_comments_and_blank_lines() {
    assert_eq!(
        rejected("# a fixture\n\n0.0 downbeat\n0.5 wrong\n"),
        AnnotationError::EntryKind { line: 4 }
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
        AnnotationError::EntryKind { line: 4 }.to_string(),
        "line 4: the entry kind is not one of 'beat', 'downbeat', 'chord' or 'note'"
    );
}

#[test]
fn an_empty_annotation_describes_itself() {
    assert_eq!(
        AnnotationError::Empty.to_string(),
        "the annotation has no beats"
    );
}

#[test]
fn an_annotation_of_beats_alone_carries_no_harmony_and_no_line() {
    let annotation = parsed(FOUR_FOUR);

    assert!(annotation.chords().is_empty());
    assert!(annotation.notes().is_empty());
}

#[test]
fn a_chord_runs_until_the_next_entry() {
    let annotation = parsed(A_BAR_OF_EACH);

    assert_eq!(
        annotation.chords()[0],
        Chord {
            label: label("C:maj"),
            from: Duration::ZERO,
            to: Duration::from_secs(2),
        }
    );
}

#[test]
fn the_last_chord_entry_ends_the_harmony_rather_than_starting_a_span() {
    let annotation = parsed(A_BAR_OF_EACH);

    assert_eq!(annotation.chords().len(), 2);
}

#[test]
fn silence_between_two_chords_is_a_span_of_its_own() {
    let annotation = parsed(&beats_of(
        "0.0 chord C:maj\n1.0 chord N\n2.0 chord A:min\n3.0 chord N\n",
    ));

    let labels: Vec<_> = annotation
        .chords()
        .iter()
        .map(|chord| chord.label)
        .collect();

    assert_eq!(labels, [label("C:maj"), ChordLabel::Silent, label("A:min")]);
}

#[test]
fn chords_that_do_not_end_in_silence_are_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 chord C:maj\n2.0 chord A:min\n")),
        AnnotationError::UnterminatedChords
    );
}

#[test]
fn a_lone_silent_entry_annotates_no_harmony_at_all() {
    let annotation = parsed(&beats_of("0.0 chord N\n"));

    assert!(annotation.chords().is_empty());
}

#[test]
fn an_unreadable_chord_label_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 chord C:sus4\n1.0 chord N\n")),
        AnnotationError::ChordLabel { line: 2 }
    );
}

#[test]
fn a_chord_entry_without_a_label_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 chord\n")),
        AnnotationError::Malformed { line: 2 }
    );
}

#[test]
fn a_chord_that_goes_backwards_is_an_error() {
    assert_eq!(
        rejected(&beats_of("2.0 chord C:maj\n1.0 chord N\n")),
        AnnotationError::OutOfOrder { line: 3 }
    );
}

#[test]
fn a_note_reads_its_pitch_and_both_ends() {
    let annotation = parsed(A_BAR_OF_EACH);

    assert_eq!(
        annotation.notes()[1],
        Note {
            pitch: 64,
            onset: Duration::from_millis(500),
            offset: Duration::from_millis(900),
        }
    );
}

#[test]
fn every_note_is_read_in_the_order_it_is_played() {
    let annotation = parsed(A_BAR_OF_EACH);

    let pitches: Vec<_> = annotation.notes().iter().map(|note| note.pitch).collect();

    assert_eq!(pitches, [60, 64]);
}

#[test]
fn a_note_may_start_where_the_one_before_it_ended() {
    let annotation = parsed(&beats_of("0.0 note 60 0.5\n0.5 note 62 1.0\n"));

    assert_eq!(annotation.notes().len(), 2);
}

#[test]
fn notes_that_sound_at_once_are_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 note 60 1.0\n0.5 note 64 1.5\n")),
        AnnotationError::Overlap { line: 3 }
    );
}

#[test]
fn a_note_that_goes_backwards_is_an_error() {
    assert_eq!(
        rejected(&beats_of("1.0 note 60 1.5\n0.5 note 64 0.8\n")),
        AnnotationError::OutOfOrder { line: 3 }
    );
}

#[test]
fn a_note_that_ends_before_it_starts_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.5 note 60 0.2\n")),
        AnnotationError::NoteSpan { line: 2 }
    );
}

#[test]
fn a_note_that_ends_where_it_starts_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.5 note 60 0.5\n")),
        AnnotationError::NoteSpan { line: 2 }
    );
}

#[test]
fn a_pitch_above_the_midi_range_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 note 128 0.5\n")),
        AnnotationError::Pitch { line: 2 }
    );
}

#[test]
fn a_pitch_that_is_not_a_number_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 note middle-c 0.5\n")),
        AnnotationError::Pitch { line: 2 }
    );
}

#[test]
fn a_note_without_an_end_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 note 60\n")),
        AnnotationError::Malformed { line: 2 }
    );
}

#[test]
fn a_note_ending_at_something_other_than_a_timestamp_is_an_error() {
    assert_eq!(
        rejected(&beats_of("0.0 note 60 later\n")),
        AnnotationError::Timestamp { line: 2 }
    );
}

#[test]
fn each_kind_keeps_its_own_order_so_they_can_be_written_in_blocks() {
    let annotation = parsed(A_BAR_OF_EACH);

    assert_eq!(annotation.beats().len(), 2);
    assert_eq!(annotation.chords().len(), 2);
    assert_eq!(annotation.notes().len(), 2);
}

#[test]
fn an_annotation_of_harmony_with_no_beats_is_an_error() {
    assert_eq!(
        rejected("0.0 chord C:maj\n2.0 chord N\n"),
        AnnotationError::Empty
    );
}

#[test]
fn unterminated_chords_report_no_line() {
    assert_eq!(AnnotationError::UnterminatedChords.line(), None);
}

#[test]
fn unterminated_chords_describe_themselves() {
    assert_eq!(
        AnnotationError::UnterminatedChords.to_string(),
        "the chord entries do not end with 'N'"
    );
}

#[test]
fn a_note_error_describes_itself_and_names_its_line() {
    assert_eq!(
        AnnotationError::Overlap { line: 9 }.to_string(),
        "line 9: the note starts before the one before it ended"
    );
}
