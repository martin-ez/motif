//! What a control shows when its event is delivered, and how it settles back.
//!
//! The count is in frames, so nothing here reads a clock. A mark is set when
//! the event reaches the application and aged once per frame drawn after that,
//! which is the whole of the decay: the numbers below are frames of the loop.

use motif::device::{Button, Control, Encoder};
use motif::ui::{ControlEvent, Marks, Turn};

/// How many frames a mark lasts, written out rather than read from
/// [`Marks::FRAMES`] so that a change to the constant fails a test instead of
/// retuning the loops that walk it.
const FRAMES: usize = 3;

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn turned(turn: Turn) -> ControlEvent {
    ControlEvent::Turned {
        encoder: Encoder::Main,
        turn,
        shifted: false,
    }
}

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
fn a_button_that_fired_is_marked() {
    let mut marks = Marks::none();

    marks.fired(pressed(Button::Play));

    assert!(marks.marked(Button::Play));
}

#[test]
fn an_encoder_that_turned_is_marked_as_a_button_is() {
    let mut marks = Marks::none();

    marks.fired(turned(Turn::Clockwise));

    assert!(marks.marked(Encoder::Main));
}

#[test]
fn firing_one_control_leaves_the_others_at_rest() {
    let mut marks = Marks::none();

    marks.fired(pressed(Button::Play));

    assert!(!marks.marked(Button::Stop));
    assert!(!marks.marked(Encoder::Main));
}

#[test]
fn a_mark_survives_every_frame_before_the_last() {
    let mut marks = Marks::none();
    marks.fired(pressed(Button::Play));

    for frame in 1..FRAMES {
        marks.age();
        assert!(marks.marked(Button::Play), "settled after {frame} frames");
    }
}

#[test]
fn a_mark_settles_back_after_the_stated_number_of_frames() {
    let mut marks = Marks::none();
    marks.fired(pressed(Button::Play));

    aged(&mut marks, FRAMES);

    assert!(!marks.marked(Button::Play));
}

#[test]
fn firing_again_starts_the_count_over() {
    let mut marks = Marks::none();
    marks.fired(pressed(Button::Play));
    aged(&mut marks, FRAMES - 1);

    marks.fired(pressed(Button::Play));
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
    marks.fired(pressed(Button::Play));
    marks.age();
    marks.fired(pressed(Button::Stop));

    aged(&mut marks, FRAMES - 1);

    assert!(!marks.marked(Button::Play));
    assert!(marks.marked(Button::Stop));
}

#[test]
fn an_encoder_at_rest_was_turned_no_way_at_all() {
    let marks = Marks::none();

    assert_eq!(marks.turn(Encoder::Main), None);
}

#[test]
fn a_marked_encoder_reports_the_way_it_was_turned() {
    let mut marks = Marks::none();

    marks.fired(turned(Turn::Anticlockwise));

    assert_eq!(marks.turn(Encoder::Main), Some(Turn::Anticlockwise));
}

#[test]
fn turning_the_other_way_replaces_the_direction_being_shown() {
    let mut marks = Marks::none();
    marks.fired(turned(Turn::Clockwise));

    marks.fired(turned(Turn::Anticlockwise));

    assert_eq!(marks.turn(Encoder::Main), Some(Turn::Anticlockwise));
}

#[test]
fn a_settled_encoder_reports_no_direction() {
    let mut marks = Marks::none();
    marks.fired(turned(Turn::Clockwise));

    aged(&mut marks, FRAMES);

    assert_eq!(marks.turn(Encoder::Main), None);
}

#[test]
fn a_pressed_button_leaves_the_encoder_without_a_direction() {
    let mut marks = Marks::none();

    marks.fired(pressed(Button::Play));

    assert_eq!(marks.turn(Encoder::Main), None);
}
