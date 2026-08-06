//! The table a scheme is: which gestures it names, what they mean, and what a
//! gesture it leaves out reaches instead.
//!
//! The rows here are the test's own, because what a scheme binds is a
//! composition question. What is stated is that the table resolves the gestures
//! it lists and nothing else, and that swapping one table for another moves
//! which gesture the page beneath it sees without the page changing.

use std::cell::RefCell;
use std::rc::Rc;

use motif::device::{Button, Encoder};
use motif::ui::{
    App, Cell, ControlEvent, Intent, Mode, Navigation, Page, Region, Scheme, Shell, Turn,
};

const MARKER: char = '*';

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn shifted(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: true,
    }
}

fn turned(turn: Turn) -> ControlEvent {
    ControlEvent::Turned {
        encoder: Encoder::Main,
        turn,
        shifted: false,
    }
}

fn home() -> Intent {
    Intent::Show(Mode::ALL[0])
}

/// What a page was handed, readable after the shell has taken it over.
#[derive(Clone, Default)]
struct Taken(Rc<RefCell<Vec<ControlEvent>>>);

impl Taken {
    fn events(&self) -> Vec<ControlEvent> {
        self.0.borrow().clone()
    }

    fn push(&self, event: ControlEvent) {
        self.0.borrow_mut().push(event);
    }
}

struct Marked {
    taken: Taken,
}

impl Page for Marked {
    fn control(&mut self, event: ControlEvent) {
        self.taken.push(event);
    }

    fn draw(&mut self, mut region: Region<'_>) {
        region.set(0, 0, Cell::new(MARKER));
    }
}

fn navigated_by(scheme: Scheme) -> (Shell, Taken) {
    let taken = Taken::default();
    let pages = Mode::ALL.map(|_| {
        Box::new(Marked {
            taken: taken.clone(),
        }) as Box<dyn Page>
    });

    (Shell::navigated_by(pages, scheme), taken)
}

#[test]
fn a_gesture_the_scheme_names_resolves_to_its_intent() {
    let scheme = Scheme::new([(pressed(Button::Up), home())]);

    assert_eq!(scheme.intent(pressed(Button::Up)), Some(home()));
}

#[test]
fn a_gesture_the_scheme_does_not_name_resolves_to_nothing() {
    let scheme = Scheme::new([(pressed(Button::Up), home())]);

    assert_eq!(scheme.intent(pressed(Button::Play)), None);
}

#[test]
fn a_scheme_with_no_bindings_navigates_nothing() {
    let scheme = Scheme::new([]);

    for button in Button::ALL {
        assert_eq!(scheme.intent(pressed(button)), None);
    }
}

#[test]
fn every_row_of_a_scheme_answers_the_gesture_it_names() {
    let bound = [Button::Up, Button::Down, Button::Left];
    let scheme = Scheme::new(bound.map(|button| (pressed(button), home())));

    for button in bound {
        assert_eq!(scheme.intent(pressed(button)), Some(home()));
    }
}

#[test]
fn a_shifted_gesture_is_not_the_unshifted_one() {
    let scheme = Scheme::new([(pressed(Button::Up), home())]);

    assert_eq!(scheme.intent(shifted(Button::Up)), None);
}

#[test]
fn an_unshifted_gesture_is_not_the_shifted_one() {
    let scheme = Scheme::new([(shifted(Button::Up), home())]);

    assert_eq!(scheme.intent(pressed(Button::Up)), None);
}

#[test]
fn an_encoder_turn_can_navigate() {
    let scheme = Scheme::new([(turned(Turn::Clockwise), home())]);

    assert_eq!(scheme.intent(turned(Turn::Clockwise)), Some(home()));
}

#[test]
fn a_turn_the_other_way_is_a_different_gesture() {
    let scheme = Scheme::new([(turned(Turn::Clockwise), home())]);

    assert_eq!(scheme.intent(turned(Turn::Anticlockwise)), None);
}

#[test]
fn a_press_is_not_a_turn() {
    let scheme = Scheme::new([(turned(Turn::Clockwise), home())]);

    assert_eq!(scheme.intent(pressed(Button::Up)), None);
}

#[test]
fn two_schemes_with_the_same_bindings_are_equal() {
    let one = Scheme::new([(pressed(Button::Up), home())]);
    let other = Scheme::new([(pressed(Button::Up), home())]);

    assert_eq!(one, other);
}

#[test]
fn two_schemes_binding_different_gestures_differ() {
    let one = Scheme::new([(pressed(Button::Up), home())]);
    let other = Scheme::new([(pressed(Button::Down), home())]);

    assert_ne!(one, other);
}

#[test]
fn a_scheme_prints_the_gestures_it_binds() {
    let scheme = Scheme::new([(pressed(Button::Up), home())]);

    assert!(format!("{scheme:?}").contains("Up"));
}

#[test]
fn the_first_scene_shows_the_looper() {
    assert_eq!(
        Scheme::scenes().intent(pressed(Button::FirstScene)),
        Some(Intent::Show(Mode::Looper))
    );
}

#[test]
fn the_second_scene_shows_the_settings() {
    assert_eq!(
        Scheme::scenes().intent(pressed(Button::SecondScene)),
        Some(Intent::Show(Mode::Settings))
    );
}

#[test]
fn every_mode_is_reached_by_one_gesture_of_the_scenes() {
    let scheme = Scheme::scenes();

    for mode in Mode::ALL {
        let reaching = Button::ALL
            .iter()
            .filter(|&&button| scheme.intent(pressed(button)) == Some(Intent::Show(mode)))
            .count();

        assert_eq!(reaching, 1, "{mode:?} is reached by {reaching} gestures");
    }
}

#[test]
fn the_scenes_leave_the_transport_to_the_page() {
    let scheme = Scheme::scenes();

    for button in [Button::Play, Button::Stop, Button::Record] {
        assert_eq!(scheme.intent(pressed(button)), None);
    }
}

#[test]
fn the_scenes_leave_the_arrows_to_the_page() {
    let scheme = Scheme::scenes();

    for button in [Button::Up, Button::Down, Button::Left, Button::Right] {
        assert_eq!(scheme.intent(pressed(button)), None);
    }
}

#[test]
fn the_scenes_leave_the_encoder_to_the_page() {
    let scheme = Scheme::scenes();

    assert_eq!(scheme.intent(turned(Turn::Clockwise)), None);
    assert_eq!(scheme.intent(turned(Turn::Anticlockwise)), None);
}

#[test]
fn a_shifted_scene_is_not_a_gesture_the_scenes_name() {
    assert_eq!(Scheme::scenes().intent(shifted(Button::FirstScene)), None);
}

#[test]
fn a_gesture_the_scheme_names_does_not_reach_the_showing_page() {
    let (mut shell, taken) = navigated_by(Scheme::new([(pressed(Button::Up), home())]));

    shell.control(pressed(Button::Up));

    assert!(taken.events().is_empty());
}

#[test]
fn a_gesture_the_scheme_does_not_name_reaches_the_showing_page() {
    let (mut shell, taken) = navigated_by(Scheme::new([(pressed(Button::Up), home())]));

    shell.control(pressed(Button::Play));

    assert_eq!(taken.events(), vec![pressed(Button::Play)]);
}

#[test]
fn replacing_the_scheme_moves_which_gesture_the_page_sees() {
    let (mut one, seen_by_one) = navigated_by(Scheme::new([(pressed(Button::Up), home())]));
    let (mut other, seen_by_other) = navigated_by(Scheme::new([(pressed(Button::Play), home())]));

    one.control(pressed(Button::Play));
    other.control(pressed(Button::Play));

    assert_eq!(seen_by_one.events(), vec![pressed(Button::Play)]);
    assert!(seen_by_other.events().is_empty());
}
