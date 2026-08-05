//! Tapping a pulse, and the grid the taps make.
//!
//! The facts worth stating are that the taps are the grid rather than a number
//! averaged out of them, that a tempo is offered only once a player has stated
//! one, and that a sequence which has gone stale or been fumbled starts again
//! instead of dragging the tempo around with it.

use motif::seq::TapTempo;

const SAMPLE_RATE: u32 = 48_000;

/// Half a second at [`SAMPLE_RATE`], which is 120 BPM.
const HALF_SECOND: u64 = 24_000;

fn tapped(intervals: &[u64]) -> TapTempo {
    let mut taps = TapTempo::new(SAMPLE_RATE);
    let mut at = HALF_SECOND;

    taps.tap(at);
    for interval in intervals {
        at += interval;
        taps.tap(at);
    }

    taps
}

fn steady(count: usize) -> TapTempo {
    tapped(&vec![HALF_SECOND; count.saturating_sub(1)])
}

#[test]
fn a_new_tap_tempo_has_nothing_tapped() {
    let taps = TapTempo::new(SAMPLE_RATE);

    assert!(taps.grid().is_empty());
    assert_eq!(taps.tempo(), None);
}

#[test]
fn the_grid_keeps_the_rate_the_taps_were_timed_against() {
    assert_eq!(TapTempo::new(SAMPLE_RATE).grid().sample_rate(), SAMPLE_RATE);
}

#[test]
fn a_tap_lands_on_the_grid_as_the_frame_it_came_in_on() {
    let taps = steady(3);

    assert_eq!(
        taps.grid().beats(),
        &[HALF_SECOND, 2 * HALF_SECOND, 3 * HALF_SECOND]
    );
}

#[test]
fn one_tap_states_no_tempo() {
    assert_eq!(steady(1).tempo(), None);
}

#[test]
fn two_taps_state_no_tempo() {
    let taps = steady(2);

    assert_eq!(taps.grid().len(), 2);
    assert_eq!(taps.tempo(), None);
}

#[test]
fn a_tempo_arrives_on_the_third_tap() {
    assert_eq!(steady(TapTempo::TAPS_TO_A_TEMPO).tempo(), Some(120.0));
}

#[test]
fn tempo_is_derived_from_the_taps() {
    let slower = tapped(&[2 * HALF_SECOND, 2 * HALF_SECOND]);

    assert_eq!(slower.tempo(), Some(60.0));
}

#[test]
fn a_sequence_that_drifts_reports_the_tempo_it_averages() {
    let drifting = tapped(&[HALF_SECOND, HALF_SECOND, HALF_SECOND + 3 * HALF_SECOND / 5]);

    assert_eq!(drifting.tempo(), Some(100.0));
}

#[test]
fn a_tap_reports_that_it_joined_the_sequence() {
    let mut taps = steady(2);

    assert!(taps.tap(3 * HALF_SECOND));
}

#[test]
fn the_first_tap_reports_that_it_started_a_sequence() {
    let mut taps = TapTempo::new(SAMPLE_RATE);

    assert!(!taps.tap(HALF_SECOND));
}

#[test]
fn a_tap_that_does_not_come_after_the_last_is_dropped() {
    let mut taps = steady(2);

    assert!(!taps.tap(2 * HALF_SECOND));
    assert!(!taps.tap(HALF_SECOND));
    assert_eq!(taps.grid().beats(), &[HALF_SECOND, 2 * HALF_SECOND]);
}

#[test]
fn a_tap_after_a_long_silence_starts_a_new_sequence() {
    let mut taps = steady(TapTempo::TAPS_TO_A_TEMPO);
    let stale = 3 * HALF_SECOND + 7 * HALF_SECOND;

    assert!(!taps.tap(stale));
    assert_eq!(taps.grid().beats(), &[stale]);
    assert_eq!(taps.tempo(), None);
}

#[test]
fn a_tap_that_lands_late_starts_a_new_sequence() {
    let mut taps = steady(TapTempo::TAPS_TO_A_TEMPO);
    let late = 3 * HALF_SECOND + 3 * HALF_SECOND;

    assert!(!taps.tap(late));
    assert_eq!(taps.grid().beats(), &[late]);
}

#[test]
fn a_tap_that_lands_early_starts_a_new_sequence() {
    let mut taps = steady(TapTempo::TAPS_TO_A_TEMPO);
    let early = 3 * HALF_SECOND + HALF_SECOND / 3;

    assert!(!taps.tap(early));
    assert_eq!(taps.grid().beats(), &[early]);
}

#[test]
fn a_tap_a_little_off_the_beat_joins_the_sequence() {
    let mut taps = steady(TapTempo::TAPS_TO_A_TEMPO);

    assert!(taps.tap(3 * HALF_SECOND + 5 * HALF_SECOND / 4));
    assert_eq!(taps.grid().len(), TapTempo::TAPS_TO_A_TEMPO + 1);
}

#[test]
fn a_second_tap_at_any_interval_joins_the_first() {
    let mut taps = steady(1);

    assert!(taps.tap(HALF_SECOND + 3 * HALF_SECOND / 2));
    assert_eq!(taps.grid().len(), 2);
}

#[test]
fn a_restarted_sequence_reaches_a_tempo_of_its_own() {
    let mut taps = steady(TapTempo::TAPS_TO_A_TEMPO);
    let restarted = 3 * HALF_SECOND + 7 * HALF_SECOND;

    taps.tap(restarted);
    taps.tap(restarted + 2 * HALF_SECOND);
    taps.tap(restarted + 4 * HALF_SECOND);

    assert_eq!(taps.tempo(), Some(60.0));
}

#[test]
fn a_second_tap_after_a_long_silence_starts_a_new_sequence() {
    let mut taps = steady(1);
    let stale = HALF_SECOND + 5 * HALF_SECOND;

    assert!(!taps.tap(stale));
    assert_eq!(taps.grid().beats(), &[stale]);
}
