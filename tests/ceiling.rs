//! The ceiling that keeps a summed loop inside full scale.
//!
//! A stack of layers is summed as it is read, so what a player hears can leave
//! full scale long before any one layer does. The ceiling is what stops it, and
//! the facts worth stating are where it starts working, what it leaves of the
//! scale once it has, and that it never turns a louder sum into a quieter one.
//!
//! The values below are binary fractions, so the curve lands on them exactly
//! and a test can state the sample rather than a tolerance around it.

use motif::audio::{HELD_ABOVE, held};

#[test]
fn silence_is_left_silent() {
    assert_eq!(held(0.0), 0.0);
}

#[test]
fn a_sum_below_the_ceiling_is_passed_through_untouched() {
    assert_eq!(held(0.5), 0.5);
}

#[test]
fn a_sum_at_the_ceiling_is_passed_through_untouched() {
    assert_eq!(held(HELD_ABOVE), HELD_ABOVE);
}

#[test]
fn a_sum_at_full_scale_is_held_short_of_it() {
    assert_eq!(held(1.0), 0.875);
}

#[test]
fn a_sum_further_past_the_ceiling_keeps_less_of_what_is_left() {
    assert_eq!(held(1.5), 0.9375);
    assert_eq!(held(2.5), 0.96875);
    assert_eq!(held(4.5), 0.984375);
}

#[test]
fn no_sum_however_large_leaves_full_scale() {
    assert_eq!(held(f32::MAX), 1.0);
}

#[test]
fn a_negative_sum_is_held_to_the_mirror_of_the_positive_one() {
    assert_eq!(held(-0.5), -0.5);
    assert_eq!(held(-1.0), -0.875);
    assert_eq!(held(-f32::MAX), -1.0);
}

#[test]
fn a_louder_sum_is_never_held_quieter() {
    let steps = 4_096;
    let sums = (0..steps).map(|step| step as f32 / 64.0);

    let quieter = sums
        .clone()
        .zip(sums.skip(1))
        .find(|&(sum, louder)| held(louder) < held(sum));

    assert_eq!(quieter, None, "a louder sum was held quieter");
}

#[test]
fn a_sum_that_is_not_a_number_is_held_at_silence() {
    assert_eq!(held(f32::NAN), 0.0);
}

#[test]
fn an_infinite_sum_is_held_at_silence() {
    assert_eq!(held(f32::INFINITY), 0.0);
    assert_eq!(held(f32::NEG_INFINITY), 0.0);
}
