//! Running a candidate over the whole fixture set: what it scores, what it
//! reports, and what it refuses to skip.

use motif::fixtures::harness::{self, GroundTruth, Report, RunError, Target};
use motif::fixtures::{AnnotationError, Chord, ChordLabel, Comparison, Note};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn unique_to_this_run(name: &str) -> String {
    format!("motif-harness-{}-{name}", std::process::id())
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(unique_to_this_run(name));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("the scratch directory is writable");
    directory
}

fn write(directory: &Path, name: &str, annotation: &str) {
    fs::write(directory.join(format!("{name}.beats")), annotation)
        .expect("the annotation is writable");
}

fn two_bars() -> &'static str {
    "0.0 downbeat\n0.5 beat\n1.0 beat\n1.5 beat\n2.0 downbeat\n2.5 beat\n3.0 beat\n3.5 beat\n"
}

fn one_bar() -> &'static str {
    "0.0 downbeat\n1.0 beat\n2.0 beat\n3.0 beat\n"
}

fn one_bar_of_each() -> String {
    format!(
        "{}0.0 chord C:maj\n2.0 chord A:min\n4.0 chord N\n0.0 note 60 0.9\n1.0 note 64 1.9\n",
        one_bar()
    )
}

fn relabelled(truth: &GroundTruth, label: &str) -> Vec<Chord> {
    let heard = ChordLabel::parse(label).expect("a chord label");
    truth
        .annotation()
        .chords()
        .iter()
        .map(|chord| Chord {
            label: heard,
            ..*chord
        })
        .collect()
}

fn exact(truth: &GroundTruth, target: Target) -> Vec<Duration> {
    target.positions(truth.annotation()).collect()
}

fn measure_over(
    directory: &Path,
    target: Target,
    candidate: impl FnMut(&GroundTruth) -> Vec<Duration>,
) -> Report {
    harness::measure(directory, target, candidate).expect("the fixtures parse")
}

#[test]
fn an_exact_candidate_scores_one_on_every_fixture() {
    let directory = scratch("exact");
    write(&directory, "a", two_bars());
    write(&directory, "b", one_bar());

    let report = measure_over(&directory, Target::Beats, |truth| {
        exact(truth, Target::Beats)
    });

    for row in report.rows() {
        assert_eq!(row.score().f1(), 1.0, "{}", row.name());
    }
    assert_eq!(report.mean(), 1.0);
}

#[test]
fn a_candidate_that_reports_nothing_scores_zero() {
    let directory = scratch("silent");
    write(&directory, "a", two_bars());

    let report = measure_over(&directory, Target::Beats, |_| Vec::new());

    assert_eq!(report.mean(), 0.0);
    assert_eq!(report.rows()[0].score().hits(), 0);
    assert_eq!(report.rows()[0].score().detected(), 0);
}

#[test]
fn a_candidate_that_finds_half_the_beats_scores_two_thirds() {
    let directory = scratch("half");
    write(&directory, "a", two_bars());

    let report = measure_over(&directory, Target::Beats, |truth| {
        exact(truth, Target::Beats).into_iter().step_by(2).collect()
    });

    let score = report.rows()[0].score();
    assert_eq!(score.hits(), 4);
    assert_eq!(score.recall(), 0.5);
    assert_eq!(score.precision(), 1.0);
    assert!((score.f1() - 2.0 / 3.0).abs() < 1e-9, "{}", score.f1());
}

#[test]
fn the_aggregate_is_the_mean_of_the_per_fixture_scores() {
    let directory = scratch("mean");
    write(&directory, "found", two_bars());
    write(&directory, "missed", one_bar());

    let report = measure_over(&directory, Target::Beats, |truth| {
        if truth.name() == "found" {
            exact(truth, Target::Beats)
        } else {
            Vec::new()
        }
    });

    assert_eq!(report.mean(), 0.5);
}

#[test]
fn downbeats_are_scored_against_fewer_positions_than_beats() {
    let directory = scratch("downbeats");
    write(&directory, "a", two_bars());

    let beats = measure_over(&directory, Target::Beats, |truth| {
        exact(truth, Target::Beats)
    });
    let downbeats = measure_over(&directory, Target::Downbeats, |truth| {
        exact(truth, Target::Downbeats)
    });

    assert_eq!(beats.rows()[0].score().annotated(), 8);
    assert_eq!(downbeats.rows()[0].score().annotated(), 2);
}

#[test]
fn a_candidate_reporting_every_beat_is_wrong_about_the_downbeats() {
    let directory = scratch("every-beat");
    write(&directory, "a", two_bars());

    let report = measure_over(&directory, Target::Downbeats, |truth| {
        exact(truth, Target::Beats)
    });

    let score = report.rows()[0].score();
    assert_eq!(score.recall(), 1.0);
    assert_eq!(score.precision(), 0.25);
}

#[test]
fn every_fixture_in_the_set_gets_a_row() {
    let directory = scratch("rows");
    write(&directory, "a", two_bars());
    write(&directory, "b", one_bar());
    write(&directory, "c", one_bar());

    let report = measure_over(&directory, Target::Beats, |_| Vec::new());

    assert_eq!(report.rows().len(), 3);
}

#[test]
fn rows_come_back_in_a_stable_order() {
    let directory = scratch("order");
    write(&directory, "zulu", one_bar());
    write(&directory, "alpha", one_bar());
    write(&directory, "mike", one_bar());

    let report = measure_over(&directory, Target::Beats, |_| Vec::new());
    let names: Vec<_> = report.rows().iter().map(|row| row.name()).collect();

    assert_eq!(names, ["alpha", "mike", "zulu"]);
}

#[test]
fn the_candidate_is_offered_each_fixtures_ground_truth() {
    let directory = scratch("offered");
    write(&directory, "a", two_bars());
    write(&directory, "b", one_bar());

    let mut seen = Vec::new();
    measure_over(&directory, Target::Beats, |truth| {
        seen.push((truth.name().to_owned(), truth.annotation().beats().len()));
        Vec::new()
    });
    seen.sort();

    assert_eq!(seen, [("a".to_owned(), 8), ("b".to_owned(), 4)]);
}

#[test]
fn a_file_that_is_not_an_annotation_is_not_a_fixture() {
    let directory = scratch("ignored");
    write(&directory, "a", one_bar());
    fs::write(directory.join("a.wav"), b"not an annotation").expect("the audio is writable");
    fs::write(directory.join("README.md"), "# prose").expect("the readme is writable");

    let report = measure_over(&directory, Target::Beats, |_| Vec::new());

    assert_eq!(report.rows().len(), 1);
}

#[test]
fn an_unparsable_annotation_fails_the_run_naming_the_fixture() {
    let directory = scratch("unparsable");
    write(&directory, "good", one_bar());
    write(&directory, "broken", "0.0 downbeat\n1.0 offbeat\n");

    let error = harness::measure(&directory, Target::Beats, |_| Vec::new())
        .expect_err("a broken annotation fails the run");

    assert!(
        matches!(&error, RunError::Annotation { fixture, .. } if fixture == "broken"),
        "{error:?}"
    );
    assert!(error.to_string().contains("broken"), "{error}");
}

#[test]
fn an_annotation_with_no_downbeats_fails_the_run() {
    let directory = scratch("no-downbeats");
    write(&directory, "flat", "0.0 beat\n1.0 beat\n");

    let error = harness::measure(&directory, Target::Downbeats, |_| Vec::new())
        .expect_err("an annotation with no downbeats fails the run");

    assert!(
        matches!(&error, RunError::Annotation { fixture, .. } if fixture == "flat"),
        "{error:?}"
    );
}

#[test]
fn a_bad_annotation_is_not_quietly_left_out_of_the_aggregate() {
    let directory = scratch("not-skipped");
    write(&directory, "good", one_bar());
    write(&directory, "broken", "nonsense\n");

    let outcome = harness::measure(&directory, Target::Beats, |truth| {
        exact(truth, Target::Beats)
    });

    assert!(outcome.is_err(), "a perfect candidate still fails the run");
}

#[test]
fn a_directory_with_no_fixtures_fails_the_run() {
    let directory = scratch("bare");

    let error = harness::measure(&directory, Target::Beats, |_| Vec::new())
        .expect_err("a set with no fixtures fails the run");

    assert!(matches!(error, RunError::Empty { .. }), "{error:?}");
}

#[test]
fn a_missing_directory_fails_the_run() {
    let directory = scratch("missing").join("nowhere");

    let error = harness::measure(&directory, Target::Beats, |_| Vec::new())
        .expect_err("a missing set fails the run");

    assert!(matches!(error, RunError::Directory { .. }), "{error:?}");
}

#[test]
fn a_broken_annotation_carries_the_parse_error_as_its_cause() {
    let directory = scratch("annotation-cause");
    write(&directory, "broken", "0.0 downbeat\n1.0 offbeat\n");

    let error = harness::measure(&directory, Target::Beats, |_| Vec::new())
        .expect_err("a broken annotation fails the run");
    let cause = error.source().expect("the parse error is the cause");

    assert_eq!(
        cause.downcast_ref::<AnnotationError>(),
        Some(&AnnotationError::EntryKind { line: 2 })
    );
}

#[test]
fn a_missing_directory_carries_the_filesystem_error_as_its_cause() {
    let directory = scratch("directory-cause").join("nowhere");

    let error = harness::measure(&directory, Target::Beats, |_| Vec::new())
        .expect_err("a missing set fails the run");
    let cause = error.source().expect("the filesystem error is the cause");

    assert_eq!(
        cause.downcast_ref::<io::Error>().map(io::Error::kind),
        Some(io::ErrorKind::NotFound)
    );
}

#[test]
fn a_set_with_no_fixtures_has_nothing_underlying_to_report() {
    let directory = scratch("empty-cause");

    let error = harness::measure(&directory, Target::Beats, |_| Vec::new())
        .expect_err("a set with no fixtures fails the run");

    assert!(error.source().is_none(), "{error:?}");
}

#[test]
fn the_report_names_every_fixture_and_its_aggregate() {
    let directory = scratch("display");
    write(&directory, "alpha", one_bar());
    write(&directory, "zulu", one_bar());

    let report = measure_over(&directory, Target::Beats, |truth| {
        exact(truth, Target::Beats)
    });
    let shown = report.to_string();

    assert!(shown.contains("alpha"), "{shown}");
    assert!(shown.contains("zulu"), "{shown}");
    assert!(shown.contains("1.000"), "{shown}");
}

#[test]
fn the_checked_in_set_scores_a_candidate_taken_from_its_own_annotations() {
    let report = harness::measure(&harness::checked_in(), Target::Downbeats, |truth| {
        exact(truth, Target::Downbeats)
    })
    .expect("the checked-in set parses");

    assert!(!report.rows().is_empty());
    assert_eq!(report.mean(), 1.0);
}

#[test]
fn a_report_of_beats_names_the_figure_it_quotes() {
    let directory = scratch("quoted-beats");
    write(&directory, "a", one_bar());

    let report = measure_over(&directory, Target::Beats, |_| Vec::new());

    assert!(report.to_string().contains("mean F1"), "{report}");
}

#[test]
fn an_exact_chord_candidate_scores_one() {
    let directory = scratch("chords-exact");
    write(&directory, "a", &one_bar_of_each());

    let report = harness::measure_chords(&directory, Comparison::Sevenths, |truth| {
        truth.annotation().chords().to_vec()
    })
    .expect("the fixtures parse");

    assert_eq!(report.mean(), 1.0);
}

#[test]
fn a_chord_candidate_that_reports_nothing_scores_zero() {
    let directory = scratch("chords-silent");
    write(&directory, "a", &one_bar_of_each());

    let report = harness::measure_chords(&directory, Comparison::Root, |_| Vec::new())
        .expect("the fixtures parse");

    assert_eq!(report.mean(), 0.0);
}

#[test]
fn a_chord_candidate_is_scored_at_the_level_it_is_given() {
    let directory = scratch("chords-level");
    write(&directory, "a", &one_bar_of_each());

    let roots = harness::measure_chords(&directory, Comparison::Root, |truth| {
        relabelled(truth, "C:min")
    })
    .expect("the fixtures parse");
    let sevenths = harness::measure_chords(&directory, Comparison::Sevenths, |truth| {
        relabelled(truth, "C:min")
    })
    .expect("the fixtures parse");

    assert_eq!(roots.mean(), 0.5);
    assert_eq!(sevenths.mean(), 0.0);
}

#[test]
fn a_report_of_chords_names_the_figure_it_quotes() {
    let directory = scratch("quoted-chords");
    write(&directory, "a", &one_bar_of_each());

    let report = harness::measure_chords(&directory, Comparison::Root, |_| Vec::new())
        .expect("the fixtures parse");

    assert!(report.to_string().contains("mean accuracy"), "{report}");
}

#[test]
fn a_fixture_that_annotates_no_harmony_is_not_in_a_chord_run() {
    let directory = scratch("chords-subset");
    write(&directory, "harmony", &one_bar_of_each());
    write(&directory, "rhythm", two_bars());

    let report = harness::measure_chords(&directory, Comparison::Root, |truth| {
        truth.annotation().chords().to_vec()
    })
    .expect("the fixtures parse");

    assert_eq!(report.rows().len(), 1);
    assert_eq!(report.rows()[0].name(), "harmony");
}

#[test]
fn a_set_that_annotates_no_harmony_fails_a_chord_run() {
    let directory = scratch("chords-none");
    write(&directory, "rhythm", two_bars());

    let error = harness::measure_chords(&directory, Comparison::Root, |_| Vec::new())
        .expect_err("a set with no harmony fails a chord run");

    assert!(matches!(error, RunError::Empty { .. }), "{error:?}");
}

#[test]
fn an_exact_note_candidate_scores_one() {
    let directory = scratch("notes-exact");
    write(&directory, "a", &one_bar_of_each());

    let report = harness::measure_notes(&directory, |truth| truth.annotation().notes().to_vec())
        .expect("the fixtures parse");

    assert_eq!(report.mean(), 1.0);
}

#[test]
fn a_note_candidate_transposed_by_a_semitone_scores_zero() {
    let directory = scratch("notes-transposed");
    write(&directory, "a", &one_bar_of_each());

    let report = harness::measure_notes(&directory, |truth| {
        truth
            .annotation()
            .notes()
            .iter()
            .map(|note| Note {
                pitch: note.pitch + 1,
                ..*note
            })
            .collect()
    })
    .expect("the fixtures parse");

    assert_eq!(report.mean(), 0.0);
}

#[test]
fn a_fixture_that_annotates_no_line_is_not_in_a_note_run() {
    let directory = scratch("notes-subset");
    write(&directory, "line", &one_bar_of_each());
    write(&directory, "rhythm", two_bars());

    let report = harness::measure_notes(&directory, |truth| truth.annotation().notes().to_vec())
        .expect("the fixtures parse");

    assert_eq!(report.rows().len(), 1);
    assert_eq!(report.rows()[0].name(), "line");
}

#[test]
fn a_set_that_annotates_no_line_fails_a_note_run() {
    let directory = scratch("notes-none");
    write(&directory, "rhythm", two_bars());

    let error = harness::measure_notes(&directory, |_| Vec::new())
        .expect_err("a set with no line fails a note run");

    assert!(matches!(error, RunError::Empty { .. }), "{error:?}");
}

#[test]
fn the_checked_in_set_scores_a_chord_candidate_taken_from_its_own_annotations() {
    let report = harness::measure_chords(&harness::checked_in(), Comparison::Sevenths, |truth| {
        truth.annotation().chords().to_vec()
    })
    .expect("the checked-in set parses");

    assert!(!report.rows().is_empty());
    assert_eq!(report.mean(), 1.0);
}

#[test]
fn the_checked_in_set_scores_a_note_candidate_taken_from_its_own_annotations() {
    let report = harness::measure_notes(&harness::checked_in(), |truth| {
        truth.annotation().notes().to_vec()
    })
    .expect("the checked-in set parses");

    assert!(!report.rows().is_empty());
    assert_eq!(report.mean(), 1.0);
}
