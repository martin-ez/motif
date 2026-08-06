//! A closed set whose variants are not all in every build.
//!
//! A screen that exists behind a Cargo feature is a mode that exists behind
//! one, and the set has to hold that without leaving a gap: a length counting
//! a variant this build does not have, or a discriminant that no longer finds
//! its own place in the order, is an index into an array that is one short.
//!
//! The set here is the test's own, gated on `frame-pace`, because the
//! instrument has no optional screen yet. What is stated is the property the
//! mode set needs of the mechanism, in the build where the feature is off and
//! in the build where it is on.

use motif::closed_set;

closed_set! {
    /// A set holding a variant only some builds have.
    enum Gated;
    /// Every variant this build has, in the order they are written.
    const ALL;
    /// Present in every build, and before the gated one.
    First,
    /// Present only where `frame-pace` is on.
    #[cfg(feature = "frame-pace")]
    Optional,
    /// Present in every build, and after the gated one.
    Last,
}

const GATED_VARIANTS: usize = 1;
const UNGATED_VARIANTS: usize = 2;

fn variants_in_this_build() -> usize {
    if cfg!(feature = "frame-pace") {
        UNGATED_VARIANTS + GATED_VARIANTS
    } else {
        UNGATED_VARIANTS
    }
}

#[test]
fn a_set_is_as_long_as_the_variants_this_build_has() {
    assert_eq!(Gated::ALL.len(), variants_in_this_build());
}

#[test]
fn a_variant_is_its_own_place_in_the_order() {
    for (position, variant) in Gated::ALL.iter().enumerate() {
        assert_eq!(*variant as usize, position);
    }
}

#[test]
fn a_variant_after_a_gated_one_moves_up_with_it() {
    assert_eq!(Gated::Last as usize, variants_in_this_build() - 1);
}

#[test]
fn no_variant_is_listed_twice() {
    for (position, variant) in Gated::ALL.iter().enumerate() {
        let duplicate = Gated::ALL[position + 1..].contains(variant);

        assert!(!duplicate, "{variant:?} is listed more than once");
    }
}

#[test]
fn the_variants_before_and_after_the_gate_are_in_every_build() {
    assert_eq!(Gated::ALL[0], Gated::First);
    assert_eq!(Gated::ALL[Gated::ALL.len() - 1], Gated::Last);
}

#[cfg(not(feature = "frame-pace"))]
#[test]
fn a_gated_variant_is_absent_where_its_feature_is_off() {
    assert_eq!(Gated::ALL, [Gated::First, Gated::Last]);
}

#[cfg(feature = "frame-pace")]
#[test]
fn a_gated_variant_is_present_where_its_feature_is_on() {
    assert_eq!(Gated::ALL, [Gated::First, Gated::Optional, Gated::Last]);
}
