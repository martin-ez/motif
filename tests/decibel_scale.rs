//! The decibel scale a level is drawn and moved on.
//!
//! One scale under the input meter's bar and under the looper's gain encoder,
//! so what is worth stating is the scale itself: where full scale sits, what a
//! decade of amplitude is worth in decibels, that the two conversions invert
//! each other, and where the bottom of the range is.

use motif::ui::{FLOOR_DBFS, amplitude, decibels};

/// Nearer than a bar cell or an encoder detent can tell apart, so two readings
/// this close are the same reading.
const TOLERANCE: f32 = 1e-4;

/// Half full scale, in decibels, to more places than [`TOLERANCE`] cares about.
const HALF_SCALE_DECIBELS: f32 = -6.020_6;

fn assert_near(measured: f32, expected: f32) {
    assert!(
        (measured - expected).abs() < TOLERANCE,
        "{measured} is not {expected}"
    );
}

#[test]
fn full_scale_is_zero_decibels() {
    assert_eq!(decibels(1.0), 0.0);
}

#[test]
fn a_tenth_of_full_scale_is_twenty_decibels_down() {
    assert_near(decibels(0.1), -20.0);
}

#[test]
fn half_the_amplitude_is_six_decibels_down() {
    assert_near(decibels(0.5), HALF_SCALE_DECIBELS);
}

#[test]
fn a_hundredth_of_full_scale_is_forty_decibels_down() {
    assert_near(decibels(0.01), -40.0);
}

#[test]
fn zero_decibels_is_unity() {
    assert_eq!(amplitude(0.0), 1.0);
}

#[test]
fn six_decibels_down_is_half_the_amplitude() {
    assert_near(amplitude(HALF_SCALE_DECIBELS), 0.5);
}

#[test]
fn twenty_decibels_up_is_ten_times_the_amplitude() {
    assert_near(amplitude(20.0), 10.0);
}

#[test]
fn the_two_conversions_invert_each_other() {
    assert_near(amplitude(decibels(0.25)), 0.25);
}

#[test]
fn the_floor_is_sixty_decibels_below_full_scale() {
    assert_eq!(FLOOR_DBFS, -60.0);
}

#[test]
fn the_floor_is_a_thousandth_of_full_scale() {
    assert_near(amplitude(FLOOR_DBFS), 0.001);
}
