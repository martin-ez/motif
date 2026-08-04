//! The input model the application sees.
//!
//! Nothing here names a key, a keyboard or a terminal, which is the property
//! the model exists to have: the panel has encoders and buttons, so that is all
//! an application can be handed.

use motif::device::{Button, Encoder};
use motif::ui::{ControlEvent, Controls, ScriptedControls, Turn};

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn turned(encoder: Encoder, turn: Turn) -> ControlEvent {
    ControlEvent::Turned {
        encoder,
        turn,
        shifted: false,
    }
}

#[test]
fn a_scripted_event_is_polled_back() {
    let mut controls = ScriptedControls::new([pressed(Button::Play)]);

    assert_eq!(controls.poll(), Some(pressed(Button::Play)));
}

#[test]
fn scripted_events_arrive_in_the_order_they_were_given() {
    let mut controls = ScriptedControls::new([pressed(Button::Play), pressed(Button::Stop)]);

    assert_eq!(controls.poll(), Some(pressed(Button::Play)));
    assert_eq!(controls.poll(), Some(pressed(Button::Stop)));
}

#[test]
fn a_source_with_nothing_left_yields_nothing() {
    let mut controls = ScriptedControls::new([pressed(Button::Play)]);
    controls.poll();

    assert_eq!(controls.poll(), None);
}

#[test]
fn a_source_that_was_never_given_anything_yields_nothing() {
    let mut controls = ScriptedControls::new([]);

    assert_eq!(controls.poll(), None);
}

#[test]
fn an_event_pushed_after_the_script_ran_out_is_polled_next() {
    let mut controls = ScriptedControls::new([pressed(Button::Play)]);
    controls.poll();

    controls.push(pressed(Button::Record));

    assert_eq!(controls.poll(), Some(pressed(Button::Record)));
}

#[test]
fn a_turn_names_the_encoder_and_which_way_it_went() {
    let event = turned(Encoder::Third, Turn::Anticlockwise);

    assert_eq!(
        event,
        ControlEvent::Turned {
            encoder: Encoder::Third,
            turn: Turn::Anticlockwise,
            shifted: false,
        }
    );
}

#[test]
fn an_unshifted_event_is_not_shifted() {
    assert!(!pressed(Button::Play).is_shifted());
    assert!(!turned(Encoder::First, Turn::Clockwise).is_shifted());
}

#[test]
fn a_shifted_press_is_shifted() {
    let event = ControlEvent::Pressed {
        button: Button::Play,
        shifted: true,
    };

    assert!(event.is_shifted());
}

#[test]
fn a_shifted_turn_is_shifted() {
    let event = ControlEvent::Turned {
        encoder: Encoder::First,
        turn: Turn::Clockwise,
        shifted: true,
    };

    assert!(event.is_shifted());
}
