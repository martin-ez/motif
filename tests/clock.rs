//! The clock the event loop paces itself against.
//!
//! Time is reached through a trait so that a test can state how long a frame
//! took instead of taking that long, and so that asserting on pacing does not
//! mean waiting for it.

use std::time::{Duration, Instant};

use motif::ui::{Clock, ScriptedClock, SystemClock};

fn milliseconds(count: u64) -> Duration {
    Duration::from_millis(count)
}

#[test]
fn a_scripted_clock_reads_back_the_times_it_was_given() {
    let mut clock = ScriptedClock::new([milliseconds(0), milliseconds(5)]);

    let started = clock.now();
    let ended = clock.now();

    assert_eq!(ended - started, milliseconds(5));
}

#[test]
fn a_scripted_clock_that_runs_out_of_script_stops_moving() {
    let mut clock = ScriptedClock::new([milliseconds(0), milliseconds(5)]);
    clock.now();
    clock.now();

    let after = clock.now();
    let later = clock.now();

    assert_eq!(later, after);
}

#[test]
fn a_scripted_clock_with_no_script_does_not_move() {
    let mut clock = ScriptedClock::new([]);

    assert_eq!(clock.now(), clock.now());
}

#[test]
fn a_scripted_clock_records_what_it_was_asked_to_sleep() {
    let mut clock = ScriptedClock::new([]);

    clock.sleep(milliseconds(7));
    clock.sleep(milliseconds(3));

    assert_eq!(clock.slept(), [milliseconds(7), milliseconds(3)]);
}

#[test]
fn a_scripted_clock_records_nothing_until_it_is_asked_to_sleep() {
    let clock = ScriptedClock::new([milliseconds(0)]);

    assert!(clock.slept().is_empty());
}

#[test]
fn a_scripted_clock_does_not_actually_wait() {
    let mut clock = ScriptedClock::new([]);

    let before = Instant::now();
    clock.sleep(Duration::from_secs(30));

    assert!(before.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_system_clock_reads_a_later_time_after_sleeping() {
    let mut clock = SystemClock::new();

    let before = clock.now();
    clock.sleep(milliseconds(1));

    assert!(clock.now() > before);
}

#[test]
fn a_system_clock_waits_at_least_as_long_as_it_was_asked_to() {
    let mut clock = SystemClock::new();

    let before = Instant::now();
    clock.sleep(milliseconds(5));

    assert!(before.elapsed() >= milliseconds(5));
}
