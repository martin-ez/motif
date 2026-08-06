//! What a control shows when its event is delivered, and how it settles back.
//!
//! The count is in frames, so nothing here reads a clock. A mark is set when
//! the event reaches the application and aged once per frame drawn after that,
//! which is the whole of the decay: the numbers below are frames of the loop.

use motif::device::{Button, Control, Encoder};
use motif::ui::Marks;

/// How many frames a mark lasts, written out rather than read from
/// [`Marks::FRAMES`] so that a change to the constant fails a test instead of
/// retuning the loops that walk it.
const FRAMES: usize = 3;

fn aged(marks: &mut Marks, frames: usize) {
    for _ in 0..frames {
        marks.age();
    }
}

#[test]
fn a_mark_lasts_three_frames() {
    assert_eq!(usize::from(Marks::FRAMES), FRAMES);
}

#[test]
fn nothing_is_marked_before_a_control_fires() {
    let marks = Marks::none();

    for control in Control::ALL {
        assert!(!marks.marked(control), "{control:?} was marked at rest");
    }
}

#[test]
fn a_control_that_fired_is_marked() {
    let mut marks = Marks::none();

    marks.fired(Button::Play);

    assert!(marks.marked(Button::Play));
}

#[test]
fn an_encoder_that_turned_is_marked_as_a_button_is() {
    let mut marks = Marks::none();

    marks.fired(Encoder::Main);

    assert!(marks.marked(Encoder::Main));
}

#[test]
fn firing_one_control_leaves_the_others_at_rest() {
    let mut marks = Marks::none();

    marks.fired(Button::Play);

    assert!(!marks.marked(Button::Stop));
    assert!(!marks.marked(Encoder::Main));
}

#[test]
fn a_mark_survives_every_frame_before_the_last() {
    let mut marks = Marks::none();
    marks.fired(Button::Play);

    for frame in 1..FRAMES {
        marks.age();
        assert!(marks.marked(Button::Play), "settled after {frame} frames");
    }
}

#[test]
fn a_mark_settles_back_after_the_stated_number_of_frames() {
    let mut marks = Marks::none();
    marks.fired(Button::Play);

    aged(&mut marks, FRAMES);

    assert!(!marks.marked(Button::Play));
}

#[test]
fn firing_again_starts_the_count_over() {
    let mut marks = Marks::none();
    marks.fired(Button::Play);
    aged(&mut marks, FRAMES - 1);

    marks.fired(Button::Play);
    aged(&mut marks, FRAMES - 1);

    assert!(marks.marked(Button::Play));
}

#[test]
fn a_control_at_rest_stays_at_rest_however_many_frames_pass() {
    let mut marks = Marks::none();

    aged(&mut marks, FRAMES * 4);

    assert!(!marks.marked(Button::Play));
}

#[test]
fn two_controls_settle_on_the_frames_they_each_fired_on() {
    let mut marks = Marks::none();
    marks.fired(Button::Play);
    marks.age();
    marks.fired(Button::Stop);

    aged(&mut marks, FRAMES - 1);

    assert!(!marks.marked(Button::Play));
    assert!(marks.marked(Button::Stop));
}
