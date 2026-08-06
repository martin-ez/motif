//! Chord labels and how two segmentations of them are compared: what a label
//! reads and writes as, and how much of a span two sides agree over.

use motif::fixtures::{Agreement, Chord, ChordLabel, Comparison, PitchClass, Quality};
use std::time::Duration;

fn label(text: &str) -> ChordLabel {
    ChordLabel::parse(text).unwrap_or_else(|| panic!("{text} is a chord label"))
}

fn spans(entries: &[(&str, u64, u64)]) -> Vec<Chord> {
    entries
        .iter()
        .map(|(text, from, to)| Chord {
            label: label(text),
            from: Duration::from_millis(*from),
            to: Duration::from_millis(*to),
        })
        .collect()
}

fn accuracy(annotated: &[Chord], detected: &[Chord], comparison: Comparison) -> f64 {
    Agreement::of(annotated, detected, comparison).accuracy()
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "{actual} is not {expected}"
    );
}

#[test]
fn a_pitch_class_is_semitones_above_c() {
    assert_eq!(PitchClass::from_semitone(7).semitone(), 7);
}

#[test]
fn a_pitch_class_wraps_at_the_octave() {
    assert_eq!(PitchClass::from_semitone(14), PitchClass::from_semitone(2));
}

#[test]
fn a_pitch_class_writes_itself_with_a_sharp() {
    assert_eq!(PitchClass::from_semitone(6).to_string(), "F#");
}

#[test]
fn a_major_triad_reads_its_root_and_quality() {
    assert_eq!(
        label("C:maj"),
        ChordLabel::Sounding(PitchClass::from_semitone(0), Quality::Maj)
    );
}

#[test]
fn a_sharp_root_reads_as_the_semitone_above() {
    assert_eq!(
        label("F#:min"),
        ChordLabel::Sounding(PitchClass::from_semitone(6), Quality::Min)
    );
}

#[test]
fn a_dominant_seventh_is_written_without_a_quality_word() {
    assert_eq!(
        label("G:7"),
        ChordLabel::Sounding(PitchClass::from_semitone(7), Quality::Dom7)
    );
}

#[test]
fn no_chord_is_written_n() {
    assert_eq!(label("N"), ChordLabel::Silent);
}

#[test]
fn every_label_round_trips_through_its_text() {
    for text in [
        "N", "C:maj", "C#:min", "D:dim", "D#:aug", "E:maj7", "F:min7", "F#:7", "B:maj",
    ] {
        assert_eq!(label(text).to_string(), text);
    }
}

#[test]
fn a_root_outside_the_twelve_is_not_a_label() {
    assert_eq!(ChordLabel::parse("H:maj"), None);
}

#[test]
fn an_unknown_quality_is_not_a_label() {
    assert_eq!(ChordLabel::parse("C:sus4"), None);
}

#[test]
fn a_label_without_a_quality_is_not_a_label() {
    assert_eq!(ChordLabel::parse("C"), None);
}

#[test]
fn an_identical_segmentation_agrees_throughout() {
    let truth = spans(&[("C:maj", 0, 1_000), ("A:min", 1_000, 2_000)]);

    assert_close(accuracy(&truth, &truth, Comparison::Sevenths), 1.0);
}

#[test]
fn a_candidate_covering_nothing_agrees_over_nothing() {
    let truth = spans(&[("C:maj", 0, 1_000)]);

    assert_close(accuracy(&truth, &[], Comparison::Root), 0.0);
}

#[test]
fn agreement_is_the_share_of_the_span_not_of_the_count() {
    let truth = spans(&[("C:maj", 0, 3_000), ("A:min", 3_000, 4_000)]);
    let detected = spans(&[("C:maj", 0, 3_000), ("F:maj", 3_000, 4_000)]);

    assert_close(accuracy(&truth, &detected, Comparison::Root), 0.75);
}

#[test]
fn a_long_wrong_chord_costs_more_than_a_short_one() {
    let truth = spans(&[("C:maj", 0, 3_000), ("A:min", 3_000, 4_000)]);
    let long_wrong = spans(&[("F:maj", 0, 3_000), ("A:min", 3_000, 4_000)]);
    let short_wrong = spans(&[("C:maj", 0, 3_000), ("F:maj", 3_000, 4_000)]);

    assert!(
        accuracy(&truth, &long_wrong, Comparison::Root)
            < accuracy(&truth, &short_wrong, Comparison::Root)
    );
}

#[test]
fn a_candidate_overlapping_only_part_of_a_span_agrees_over_that_part() {
    let truth = spans(&[("C:maj", 0, 1_000)]);
    let detected = spans(&[("C:maj", 500, 1_500)]);

    assert_close(accuracy(&truth, &detected, Comparison::Sevenths), 0.5);
}

#[test]
fn a_candidate_reaching_beyond_the_ground_truth_is_not_credited_for_it() {
    let truth = spans(&[("C:maj", 0, 1_000)]);
    let detected = spans(&[("C:maj", 0, 9_000)]);

    let agreement = Agreement::of(&truth, &detected, Comparison::Root);

    assert_eq!(agreement.agreed(), Duration::from_secs(1));
    assert_eq!(agreement.total(), Duration::from_secs(1));
}

#[test]
fn overlapping_candidate_spans_cannot_agree_over_more_than_the_span() {
    let truth = spans(&[("C:maj", 0, 1_000)]);
    let detected = spans(&[("C:maj", 0, 1_000), ("C:maj", 0, 1_000)]);

    assert_close(accuracy(&truth, &detected, Comparison::Root), 1.0);
}

#[test]
fn a_span_that_ends_before_it_starts_covers_nothing() {
    let truth = spans(&[("C:maj", 1_000, 0)]);

    let agreement = Agreement::of(&truth, &truth, Comparison::Root);

    assert_eq!(agreement.total(), Duration::ZERO);
    assert_close(agreement.accuracy(), 0.0);
}

#[test]
fn ground_truth_covering_nothing_agrees_over_nothing() {
    let detected = spans(&[("C:maj", 0, 1_000)]);

    assert_close(accuracy(&[], &detected, Comparison::Root), 0.0);
}

#[test]
fn a_major_seventh_called_a_minor_keeps_its_root() {
    let truth = spans(&[("C:maj7", 0, 1_000)]);
    let detected = spans(&[("C:min", 0, 1_000)]);

    assert_close(accuracy(&truth, &detected, Comparison::Root), 1.0);
    assert_close(accuracy(&truth, &detected, Comparison::Thirds), 0.0);
}

#[test]
fn a_chord_called_a_tritone_away_keeps_nothing() {
    let truth = spans(&[("C:maj", 0, 1_000)]);
    let detected = spans(&[("F#:maj", 0, 1_000)]);

    assert_close(accuracy(&truth, &detected, Comparison::Root), 0.0);
}

#[test]
fn a_major_seventh_called_a_major_keeps_its_third() {
    let truth = spans(&[("C:maj7", 0, 1_000)]);
    let detected = spans(&[("C:maj", 0, 1_000)]);

    assert_close(accuracy(&truth, &detected, Comparison::Thirds), 1.0);
    assert_close(accuracy(&truth, &detected, Comparison::Sevenths), 0.0);
}

#[test]
fn a_diminished_triad_has_a_minor_third() {
    assert!(Comparison::Thirds.agree(label("B:dim"), label("B:min")));
}

#[test]
fn an_augmented_triad_has_a_major_third() {
    assert!(Comparison::Thirds.agree(label("C:aug"), label("C:maj")));
}

#[test]
fn a_dominant_seventh_and_a_major_seventh_share_a_third_and_not_a_quality() {
    assert!(Comparison::Thirds.agree(label("G:7"), label("G:maj7")));
    assert!(!Comparison::Sevenths.agree(label("G:7"), label("G:maj7")));
}

#[test]
fn a_root_alone_agrees_where_the_qualities_differ() {
    assert!(Comparison::Root.agree(label("C:maj7"), label("C:min")));
}

#[test]
fn silence_called_a_chord_agrees_at_no_level() {
    for comparison in [Comparison::Root, Comparison::Thirds, Comparison::Sevenths] {
        assert!(!comparison.agree(ChordLabel::Silent, label("C:maj")));
    }
}

#[test]
fn silence_heard_as_silence_agrees_at_every_level() {
    for comparison in [Comparison::Root, Comparison::Thirds, Comparison::Sevenths] {
        assert!(comparison.agree(ChordLabel::Silent, ChordLabel::Silent));
    }
}

#[test]
fn a_chord_called_silent_agrees_at_no_level() {
    for comparison in [Comparison::Root, Comparison::Thirds, Comparison::Sevenths] {
        assert!(!comparison.agree(label("C:maj"), ChordLabel::Silent));
    }
}

#[test]
fn an_agreement_reports_how_much_of_the_span_it_counted() {
    let truth = spans(&[("C:maj", 0, 1_000), ("A:min", 1_000, 3_000)]);
    let detected = spans(&[("C:maj", 0, 1_000)]);

    let agreement = Agreement::of(&truth, &detected, Comparison::Root);

    assert_eq!(agreement.agreed(), Duration::from_secs(1));
    assert_eq!(agreement.total(), Duration::from_secs(3));
}

#[test]
fn an_agreement_describes_what_it_scored() {
    let truth = spans(&[("C:maj", 0, 1_000)]);

    let shown = Agreement::of(&truth, &truth, Comparison::Root).to_string();

    assert!(shown.contains("1.000"), "{shown}");
}
