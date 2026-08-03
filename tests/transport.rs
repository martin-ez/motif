//! What the looper's transport does when a player presses a button.
//!
//! Every state gets a test for each of the three actions, so the transition
//! table is stated here in full: fifteen facts, one per test, and a table with
//! a gap in it is a table missing a test. The rest are the two questions the
//! callback asks of a state — whether this block is captured, and whether it is
//! played — and the transitions being computable at compile time, which is what
//! allocation-free means when it is a proof rather than a measurement.

use motif::looper::Transport;

#[test]
fn a_looper_starts_with_nothing_recorded() {
    assert_eq!(Transport::default(), Transport::Idle);
}

#[test]
fn record_starts_the_first_take() {
    assert_eq!(Transport::Idle.record(), Transport::Recording);
}

#[test]
fn play_does_nothing_with_nothing_recorded() {
    assert_eq!(Transport::Idle.play(), Transport::Idle);
}

#[test]
fn stop_does_nothing_with_nothing_recorded() {
    assert_eq!(Transport::Idle.stop(), Transport::Idle);
}

#[test]
fn record_closes_the_take_and_layers_onto_it() {
    assert_eq!(Transport::Recording.record(), Transport::Overdubbing);
}

#[test]
fn play_closes_the_take_and_plays_it() {
    assert_eq!(Transport::Recording.play(), Transport::Playing);
}

#[test]
fn stop_closes_the_take_and_halts() {
    assert_eq!(Transport::Recording.stop(), Transport::Stopped);
}

#[test]
fn record_layers_onto_a_playing_loop() {
    assert_eq!(Transport::Playing.record(), Transport::Overdubbing);
}

#[test]
fn play_leaves_a_playing_loop_running() {
    assert_eq!(Transport::Playing.play(), Transport::Playing);
}

#[test]
fn stop_halts_a_playing_loop() {
    assert_eq!(Transport::Playing.stop(), Transport::Stopped);
}

#[test]
fn record_drops_out_of_a_layer() {
    assert_eq!(Transport::Overdubbing.record(), Transport::Playing);
}

#[test]
fn play_drops_out_of_a_layer() {
    assert_eq!(Transport::Overdubbing.play(), Transport::Playing);
}

#[test]
fn stop_halts_a_layer() {
    assert_eq!(Transport::Overdubbing.stop(), Transport::Stopped);
}

#[test]
fn record_resumes_a_halted_loop_as_a_layer() {
    assert_eq!(Transport::Stopped.record(), Transport::Overdubbing);
}

#[test]
fn play_resumes_a_halted_loop() {
    assert_eq!(Transport::Stopped.play(), Transport::Playing);
}

#[test]
fn stop_leaves_a_halted_loop_halted() {
    assert_eq!(Transport::Stopped.stop(), Transport::Stopped);
}

#[test]
fn record_is_its_own_inverse_once_a_loop_exists() {
    assert_eq!(Transport::Playing.record().record(), Transport::Playing);
}

#[test]
fn a_take_that_was_halted_is_still_there_to_play() {
    let halted = Transport::Idle.record().stop();

    assert_eq!(halted.play(), Transport::Playing);
}

#[test]
fn a_take_is_captured_while_it_is_recorded() {
    assert!(Transport::Recording.captures_input());
}

#[test]
fn a_layer_is_captured_while_the_loop_plays() {
    assert!(Transport::Overdubbing.captures_input());
    assert!(Transport::Overdubbing.plays_loop());
}

#[test]
fn a_playing_loop_captures_nothing() {
    assert!(!Transport::Playing.captures_input());
    assert!(Transport::Playing.plays_loop());
}

#[test]
fn the_first_take_plays_nothing_underneath_itself() {
    assert!(!Transport::Recording.plays_loop());
}

#[test]
fn a_halted_transport_neither_captures_nor_plays() {
    assert!(!Transport::Idle.captures_input());
    assert!(!Transport::Idle.plays_loop());
    assert!(!Transport::Stopped.captures_input());
    assert!(!Transport::Stopped.plays_loop());
}

#[test]
fn a_transition_is_computed_without_allocating() {
    const FIRST_TAKE: Transport = Transport::Idle.record();
    const LAYERED: Transport = FIRST_TAKE.record();
    const HALTED: Transport = LAYERED.stop();

    assert_eq!(HALTED, Transport::Stopped);
}
