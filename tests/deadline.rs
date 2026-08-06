//! The deadline analysis answers by: what a take's length sets it to, and what
//! the harness reports against it.

use motif::fixtures::Annotation;
use motif::fixtures::harness::{self, GroundTruth, Report, Row, Target};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const PAUSE: Duration = Duration::from_millis(20);

fn unique_to_this_run(name: &str) -> String {
    format!("motif-deadline-{}-{name}", std::process::id())
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(unique_to_this_run(name));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("the scratch directory is writable");
    directory
}

fn a_take_of(seconds: f64) -> String {
    format!("0.0 downbeat\n{seconds:.3} beat\n")
}

fn write(directory: &Path, name: &str, annotation: &str) {
    fs::write(directory.join(format!("{name}.beats")), annotation)
        .expect("the annotation is writable");
}

fn measure_over(directory: &Path, candidate: impl FnMut(&GroundTruth) -> Vec<Duration>) -> Report {
    harness::measure(directory, Target::Beats, candidate).expect("the fixtures parse")
}

fn row<'a>(report: &'a Report, name: &str) -> &'a Row {
    report
        .rows()
        .iter()
        .find(|row| row.name() == name)
        .expect("the fixture was scored")
}

fn dawdle(_: &GroundTruth) -> Vec<Duration> {
    thread::sleep(PAUSE);
    Vec::new()
}

fn nothing(_: &GroundTruth) -> Vec<Duration> {
    Vec::new()
}

#[test]
fn the_deadline_is_half_the_take() {
    assert_eq!(
        harness::deadline(Duration::from_secs(4)),
        Duration::from_secs(2)
    );
}

#[test]
fn a_takes_span_runs_from_its_start_to_its_last_beat() {
    let annotation: Annotation = "0.5 downbeat\n1.0 beat\n1.5 beat\n"
        .parse()
        .expect("the annotation is well formed");

    assert_eq!(annotation.span(), Duration::from_millis(1_500));
}

#[test]
fn a_fixture_carries_the_time_its_candidate_took() {
    let directory = scratch("elapsed");
    write(&directory, "a", &a_take_of(4.0));

    let report = measure_over(&directory, dawdle);

    let elapsed = row(&report, "a").elapsed();
    assert!(elapsed >= PAUSE, "{elapsed:?}");
}

#[test]
fn a_fixture_is_given_the_deadline_its_own_length_sets() {
    let directory = scratch("own-length");
    write(&directory, "brief", &a_take_of(1.0));
    write(&directory, "long", &a_take_of(8.0));

    let report = measure_over(&directory, nothing);

    assert_eq!(
        row(&report, "brief").deadline(),
        harness::deadline(Duration::from_secs(1))
    );
    assert_eq!(
        row(&report, "long").deadline(),
        harness::deadline(Duration::from_secs(8))
    );
}

#[test]
fn the_headroom_a_report_quotes_is_the_tightest_of_its_fixtures() {
    let directory = scratch("tightest");
    write(&directory, "brief", &a_take_of(1.0));
    write(&directory, "long", &a_take_of(8.0));

    let report = measure_over(&directory, nothing);

    assert_eq!(report.headroom(), row(&report, "brief").headroom());
}

#[test]
fn a_candidate_slower_than_its_deadline_leaves_no_headroom() {
    let directory = scratch("missed");
    write(&directory, "brief", &a_take_of(0.02));

    let report = measure_over(&directory, dawdle);

    assert_eq!(row(&report, "brief").headroom(), Duration::ZERO);
}

#[test]
fn a_candidate_inside_its_deadline_leaves_what_it_did_not_spend() {
    let directory = scratch("kept");
    write(&directory, "long", &a_take_of(8.0));

    let report = measure_over(&directory, dawdle);

    let kept = row(&report, "long").headroom();
    assert!(kept < harness::deadline(Duration::from_secs(8)), "{kept:?}");
    assert!(kept > Duration::from_secs(3), "{kept:?}");
}

#[test]
fn the_report_shows_what_each_fixture_took_against_its_deadline() {
    let directory = scratch("shown");
    write(&directory, "alpha", &a_take_of(4.0));

    let shown = measure_over(&directory, nothing).to_string();

    assert!(shown.contains("took"), "{shown}");
    assert!(shown.contains("headroom"), "{shown}");
}

#[test]
fn the_checked_in_set_is_analysed_inside_its_deadline() {
    let report = harness::measure(&harness::checked_in(), Target::Beats, |truth| {
        Target::Beats.positions(truth.annotation()).collect()
    })
    .expect("the checked-in set parses");

    assert!(report.headroom() > Duration::ZERO, "{report}");
}
