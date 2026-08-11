//! What a player states about how a take divides: how many bars it runs, and
//! how many beats go to each of them.
//!
//! The facts worth stating are that neither half of a count may be zero, and
//! that the two halves are kept apart rather than folded into one number.

use motif::seq::Bars;

const FOUR_BARS: usize = 4;
const THREE_FOUR: usize = 3;
const FOUR_FOUR: usize = 4;

#[test]
fn a_count_holds_the_bars_and_the_beats_it_was_stated_with() {
    let bars = Bars::of(FOUR_BARS, THREE_FOUR).expect("four bars of three beats is a count");

    assert_eq!(bars.count(), FOUR_BARS);
    assert_eq!(bars.beats_each(), THREE_FOUR);
}

#[test]
fn a_take_of_no_bars_is_not_a_count() {
    assert_eq!(Bars::of(0, FOUR_FOUR), None);
}

#[test]
fn a_bar_of_no_beats_is_not_a_count() {
    assert_eq!(Bars::of(FOUR_BARS, 0), None);
}

#[test]
fn a_count_past_what_it_crosses_the_queue_in_is_refused() {
    assert_eq!(Bars::of(Bars::MOST + 1, FOUR_FOUR), None);
    assert_eq!(Bars::of(FOUR_BARS, Bars::MOST + 1), None);
    assert_eq!(Bars::of(usize::MAX, FOUR_FOUR), None);
}

#[test]
fn a_count_of_the_most_it_crosses_in_is_a_count() {
    let bars = Bars::of(Bars::MOST, Bars::MOST).expect("the widest count still fits");

    assert_eq!(bars.count(), Bars::MOST);
    assert_eq!(bars.beats_each(), Bars::MOST);
}
