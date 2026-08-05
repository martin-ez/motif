//! The screens the instrument has: that the set is closed, that it holds only
//! screens that exist, and that its order is the order it is written in.

use motif::ui::Mode;

#[test]
fn no_mode_is_listed_twice() {
    for (position, mode) in Mode::ALL.iter().enumerate() {
        let duplicate = Mode::ALL[position + 1..].contains(mode);

        assert!(!duplicate, "{mode:?} is listed more than once");
    }
}

#[test]
fn a_mode_is_its_own_place_in_the_order() {
    for (position, mode) in Mode::ALL.iter().enumerate() {
        assert_eq!(*mode as usize, position);
    }
}

#[test]
fn the_instrument_has_one_screen_today() {
    assert_eq!(Mode::ALL, [Mode::Looper]);
}
