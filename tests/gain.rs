//! The first parameter a player controls, and the shape every one after it
//! inherits: a level that moves to where it was asked for rather than jumping.
//!
//! What a test can state about smoothing is not "it sounds right" but the two
//! facts that make it sound right — no step between one sample and the next is
//! larger than the ramp allows, and the target is reached exactly and on time.
//! Both are checked against a block of ones, where the output samples are the
//! gain itself.

use motif::audio::Gain;

const SAMPLE_RATE: u32 = 48_000;
const RAMP_FRAMES: usize = (SAMPLE_RATE as usize * 10) / 1_000;
const HALF: f32 = 0.5;
const TOLERANCE: f32 = 1e-6;
const CEILING_DECIBELS: f32 = 12.0;
const DECIBELS_PER_DECADE: f32 = 20.0;
const FAR_ABOVE_THE_CEILING: f32 = 1_000.0;

fn prepared() -> Gain {
    let mut gain = Gain::unity();
    gain.prepare(SAMPLE_RATE);

    gain
}

fn ones(frames: usize) -> Vec<f32> {
    vec![1.0; frames]
}

fn applied(gain: &mut Gain, frames: usize) -> Vec<f32> {
    let mut block = ones(frames);
    gain.apply(&mut block);

    block
}

fn settled(gain: &mut Gain) -> f32 {
    let block = applied(gain, RAMP_FRAMES * 2);

    block[block.len() - 1]
}

fn largest_step(block: &[f32]) -> f32 {
    block
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_new_gain_is_unity() {
    assert_eq!(Gain::unity().target(), 1.0);
}

#[test]
fn a_new_gain_is_not_muted() {
    assert!(!Gain::unity().muted());
}

#[test]
fn unity_gain_leaves_a_block_as_it_found_it() {
    assert_eq!(applied(&mut prepared(), 8), ones(8));
}

#[test]
fn an_empty_block_is_left_alone() {
    let mut gain = prepared();
    let mut block: Vec<f32> = Vec::new();

    gain.apply(&mut block);

    assert!(block.is_empty());
}

#[test]
fn a_gain_that_was_set_is_the_target() {
    let mut gain = prepared();

    gain.set_target(HALF);

    assert_eq!(gain.target(), HALF);
}

#[test]
fn a_gain_reaches_its_target_after_the_ramp() {
    let mut gain = prepared();
    gain.set_target(HALF);

    let block = applied(&mut gain, RAMP_FRAMES + 1);

    assert!((block[block.len() - 1] - HALF).abs() < TOLERANCE);
}

#[test]
fn a_gain_stays_at_its_target_once_it_arrives() {
    let mut gain = prepared();
    gain.set_target(HALF);
    applied(&mut gain, RAMP_FRAMES);

    let block = applied(&mut gain, 8);

    assert!(block.iter().all(|sample| (sample - HALF).abs() < TOLERANCE));
}

#[test]
fn a_gain_does_not_jump_to_its_target() {
    let mut gain = prepared();
    gain.set_target(0.0);

    let block = applied(&mut gain, RAMP_FRAMES);

    assert!(largest_step(&block) < 1.0 / RAMP_FRAMES as f32 + TOLERANCE);
}

#[test]
fn a_gain_moves_towards_its_target_from_the_block_it_was_set_in() {
    let mut gain = prepared();
    gain.set_target(0.0);

    let block = applied(&mut gain, 8);

    assert!(block[7] < block[0]);
}

#[test]
fn a_change_takes_the_same_time_however_far_it_goes() {
    let mut near = prepared();
    near.set_target(0.9);
    let mut far = prepared();
    far.set_target(0.0);

    let arrived = |gain: &mut Gain, target: f32| {
        applied(gain, RAMP_FRAMES * 2)
            .iter()
            .position(|sample| (sample - target).abs() < TOLERANCE)
    };

    assert_eq!(arrived(&mut near, 0.9), arrived(&mut far, 0.0));
}

#[test]
fn muting_ramps_down_rather_than_cutting() {
    let mut gain = prepared();

    gain.set_muted(true);
    let block = applied(&mut gain, RAMP_FRAMES + 1);

    assert!(largest_step(&block) < 1.0 / RAMP_FRAMES as f32 + TOLERANCE);
    assert!(block[block.len() - 1].abs() < TOLERANCE);
}

#[test]
fn a_muted_gain_says_it_is_muted() {
    let mut gain = prepared();

    gain.set_muted(true);

    assert!(gain.muted());
}

#[test]
fn unmuting_returns_to_the_gain_that_was_set() {
    let mut gain = prepared();
    gain.set_target(HALF);
    gain.set_muted(true);
    settled(&mut gain);

    gain.set_muted(false);

    assert!((settled(&mut gain) - HALF).abs() < TOLERANCE);
}

#[test]
fn a_gain_set_while_muted_is_what_unmuting_arrives_at() {
    let mut gain = prepared();
    gain.set_muted(true);
    settled(&mut gain);

    gain.set_target(HALF);
    assert!(settled(&mut gain).abs() < TOLERANCE);

    gain.set_muted(false);
    assert!((settled(&mut gain) - HALF).abs() < TOLERANCE);
}

#[test]
fn muting_keeps_the_target_it_was_asked_for() {
    let mut gain = prepared();
    gain.set_target(HALF);

    gain.set_muted(true);

    assert_eq!(gain.target(), HALF);
}

#[test]
fn a_gain_that_was_never_prepared_arrives_at_once() {
    let mut gain = Gain::unity();
    gain.set_target(HALF);

    let block = applied(&mut gain, 2);

    assert!((block[1] - HALF).abs() < TOLERANCE);
}

#[test]
fn a_device_granting_no_sample_rate_leaves_a_usable_gain() {
    let mut gain = Gain::unity();
    gain.prepare(0);
    gain.set_target(HALF);

    assert!(settled(&mut gain).is_finite());
}

#[test]
fn a_target_that_is_not_a_number_is_refused() {
    let mut gain = prepared();
    gain.set_target(HALF);

    gain.set_target(f32::NAN);

    assert_eq!(gain.target(), HALF);
    assert!(settled(&mut gain).is_finite());
}

#[test]
fn a_negative_target_is_taken_as_silence() {
    let mut gain = prepared();

    gain.set_target(-1.0);

    assert_eq!(gain.target(), 0.0);
}

#[test]
fn the_ceiling_is_twelve_decibels_above_unity() {
    let twelve_decibels = 10.0_f32.powf(CEILING_DECIBELS / DECIBELS_PER_DECADE);

    assert!((Gain::CEILING - twelve_decibels).abs() < TOLERANCE);
}

#[test]
fn a_target_above_the_ceiling_is_taken_as_the_ceiling() {
    let mut gain = prepared();

    gain.set_target(FAR_ABOVE_THE_CEILING);

    assert_eq!(gain.target(), Gain::CEILING);
}

#[test]
fn a_gain_never_multiplies_by_more_than_the_ceiling() {
    let mut gain = prepared();

    gain.set_target(FAR_ABOVE_THE_CEILING);

    assert!((settled(&mut gain) - Gain::CEILING).abs() < TOLERANCE);
}

#[test]
fn a_target_at_the_ceiling_is_kept() {
    let mut gain = prepared();

    gain.set_target(Gain::CEILING);

    assert_eq!(gain.target(), Gain::CEILING);
}
