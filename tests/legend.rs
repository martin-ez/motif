//! Which controls a page answers, and the panel the screen draws from that.
//!
//! The legend is where a page's declaration and a backend's glyphs meet, so this
//! is the one thing that has to hold both without either knowing the other. No
//! key is named here: the panels below hand out glyphs of their own invention,
//! which is all a legend ever learns about how a control is reached.
//!
//! The drawing is checked by where things land rather than by the text of a
//! row: a key is an edge with a glyph inside it, and the arrangement of those
//! keys is the picture of the panel.

use motif::device::{Button, Control, DeviceProfile, Encoder, ScreenProfile};
use motif::ui::{ControlEvent, Controls, Frame, Hint, Legend};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const LIGHT: char = '─';
const LIGHT_WALL: char = '│';
const HEAVY: char = '━';
const HEAVY_WALL: char = '┃';

/// The glyph the panels below reach `control` by.
fn glyph_of(control: impl Into<Control>) -> char {
    let control = control.into();
    let position = Control::ALL
        .iter()
        .position(|listed| *listed == control)
        .expect("every control is listed in ALL");

    char::from(b'A' + position as u8)
}

/// A panel reached by glyphs, as a terminal's keyboard is.
struct Lettered;

impl Controls for Lettered {
    fn poll(&mut self) -> Option<ControlEvent> {
        None
    }

    fn hint(&self, control: Control) -> Option<Hint> {
        Some(Hint::new([glyph_of(control)]))
    }
}

/// A panel whose keys are labelled under the player's hands, so the screen has
/// nothing to name them with.
struct Unlabelled;

impl Controls for Unlabelled {
    fn poll(&mut self) -> Option<ControlEvent> {
        None
    }
}

fn drawn(legend: &Legend, panel: &impl Controls) -> Frame {
    let mut frame = Frame::blank();
    legend.draw(&mut frame, panel);

    frame
}

fn row_of(frame: &Frame, row: usize) -> String {
    (0..SCREEN.columns)
        .filter_map(|column| frame.get(column, row))
        .map(|cell| cell.glyph())
        .collect()
}

fn text_of(frame: &Frame) -> String {
    (0..SCREEN.rows)
        .map(|row| row_of(frame, row))
        .collect::<Vec<_>>()
        .join("\n")
}

fn glyph_at(frame: &Frame, column: usize, row: usize) -> char {
    frame
        .get(column, row)
        .map(|cell| cell.glyph())
        .unwrap_or_default()
}

struct At {
    row: usize,
    column: usize,
}

/// Where `text` was drawn, counted in cells rather than bytes: the panel is
/// mostly box-drawing characters, which are three bytes each and one cell each.
fn found(frame: &Frame, text: &str) -> At {
    (0..SCREEN.rows)
        .find_map(|row| {
            let drawn = row_of(frame, row);
            let at = drawn.find(text)?;
            Some(At {
                row,
                column: drawn[..at].chars().count(),
            })
        })
        .unwrap_or_else(|| panic!("{text:?} is nowhere on the screen"))
}

fn key_of(frame: &Frame, control: impl Into<Control>) -> At {
    found(frame, &String::from(glyph_of(control)))
}

#[test]
fn a_page_declares_the_controls_it_answers() {
    let legend = Legend::blank().answering(Button::Play);

    assert!(legend.answers(Button::Play));
}

#[test]
fn a_control_a_page_leaves_alone_is_not_answered() {
    let legend = Legend::blank().answering(Button::Play);

    assert!(!legend.answers(Button::Up));
    assert!(!legend.answers(Encoder::Main));
    assert!(!legend.answers(Button::Shift));
}

#[test]
fn answering_a_control_twice_answers_it_once() {
    let legend = Legend::blank()
        .answering(Encoder::Main)
        .answering(Encoder::Main);

    assert!(legend.answers(Encoder::Main));
}

#[test]
fn every_key_wears_the_glyph_that_reaches_it_answered_or_not() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let text = text_of(&frame);

    for control in Control::ALL {
        assert!(
            text.contains(glyph_of(control)),
            "{control:?} is on the panel and unnamed on the screen"
        );
    }
}

#[test]
fn nothing_but_the_glyph_is_written_on_a_key() {
    let frame = drawn(&Legend::blank().answering(Button::Play), &Lettered);
    let key = key_of(&frame, Button::Play);

    assert_eq!(glyph_at(&frame, key.column - 1, key.row), ' ');
    assert_eq!(glyph_at(&frame, key.column + 1, key.row), ' ');
}

#[test]
fn a_control_the_page_answers_is_drawn_with_a_heavy_edge() {
    let frame = drawn(&Legend::blank().answering(Button::Play), &Lettered);
    let key = key_of(&frame, Button::Play);

    assert_eq!(glyph_at(&frame, key.column, key.row - 1), HEAVY);
    assert_eq!(glyph_at(&frame, key.column, key.row + 1), HEAVY);
    assert_eq!(glyph_at(&frame, key.column - 2, key.row), HEAVY_WALL);
    assert_eq!(glyph_at(&frame, key.column + 2, key.row), HEAVY_WALL);
}

#[test]
fn a_control_the_page_does_not_answer_is_drawn_light_rather_than_dropped() {
    let frame = drawn(&Legend::blank().answering(Button::Play), &Lettered);
    let key = key_of(&frame, Button::Stop);

    assert_eq!(
        glyph_at(&frame, key.column, key.row),
        glyph_of(Button::Stop)
    );
    assert_eq!(glyph_at(&frame, key.column, key.row - 1), LIGHT);
    assert_eq!(glyph_at(&frame, key.column - 2, key.row), LIGHT_WALL);
}

#[test]
fn a_navigation_key_shows_its_arrow_on_a_page_that_ignores_it() {
    let frame = drawn(&Legend::blank(), &Lettered);

    for arrow in [Button::Up, Button::Down, Button::Left, Button::Right] {
        let key = key_of(&frame, arrow);

        assert_eq!(glyph_at(&frame, key.column, key.row), glyph_of(arrow));
    }
}

#[test]
fn a_panel_that_labels_its_own_keys_still_says_which_are_live() {
    let frame = drawn(&Legend::blank().answering(Button::Play), &Unlabelled);
    let text = text_of(&frame);

    assert!(text.contains(HEAVY));
    for control in Control::ALL {
        assert!(
            !text.contains(glyph_of(control)),
            "{control:?} was named by a glyph the panel does not have"
        );
    }
}

#[test]
fn the_navigation_keys_are_drawn_in_the_pattern_they_sit_in() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let (up, down) = (key_of(&frame, Button::Up), key_of(&frame, Button::Down));
    let (left, right) = (key_of(&frame, Button::Left), key_of(&frame, Button::Right));

    assert_eq!(up.column, down.column);
    assert!(up.row < down.row);
    assert_eq!(left.row, down.row);
    assert_eq!(right.row, down.row);
    assert!(left.column < down.column);
    assert!(down.column < right.column);
}

#[test]
fn the_scene_buttons_run_left_to_right_in_a_row_of_their_own() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let scenes = [
        Button::FirstScene,
        Button::SecondScene,
        Button::ThirdScene,
        Button::FourthScene,
    ]
    .map(|scene| key_of(&frame, scene));

    for pair in scenes.windows(2) {
        assert_eq!(pair[0].row, pair[1].row);
        assert!(pair[0].column < pair[1].column);
    }
}

#[test]
fn the_action_keys_are_drawn_under_the_scene_buttons() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let scene = key_of(&frame, Button::FirstScene);
    let actions = [Button::Play, Button::Stop, Button::Record].map(|action| key_of(&frame, action));

    assert_eq!(actions[0].column, scene.column);
    for action in &actions {
        assert_eq!(action.row, scene.row + 3);
    }
}

#[test]
fn shift_is_a_key_on_the_panel_like_any_other() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let shift = key_of(&frame, Button::Shift);
    let record = key_of(&frame, Button::Record);

    assert_eq!(shift.row, record.row);
    assert!(shift.column > record.column);
}

#[test]
fn the_encoder_is_drawn_as_a_knob_rather_than_a_key() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let knob = key_of(&frame, Encoder::Main);
    let (opens, closes) = (found(&frame, "╭"), found(&frame, "╯"));

    assert_eq!(opens.row, knob.row - 1);
    assert!(opens.column < knob.column);
    assert_eq!(closes.row, knob.row + 1);
    assert!(closes.column > knob.column);
}

#[test]
fn a_knob_the_page_answers_is_drawn_doubled_rather_than_heavy() {
    let frame = drawn(&Legend::blank().answering(Encoder::Main), &Lettered);
    let knob = key_of(&frame, Encoder::Main);

    assert_eq!(glyph_at(&frame, knob.column, knob.row - 1), '═');
    assert_eq!(glyph_at(&frame, knob.column, knob.row + 1), '═');
}

#[test]
fn the_legend_keeps_to_the_rows_it_reserves() {
    let frame = drawn(&Legend::blank(), &Lettered);

    for row in 0..SCREEN.rows - Legend::ROWS {
        assert_eq!(row_of(&frame, row).trim(), "", "row {row} was drawn on");
    }
}

#[test]
fn a_key_keeps_its_place_whether_it_is_live_or_dead() {
    let all = drawn(&Legend::blank(), &Lettered);
    let one = drawn(&Legend::blank().answering(Button::Record), &Lettered);
    let (before, after) = (key_of(&all, Button::Record), key_of(&one, Button::Record));

    assert_eq!((before.row, before.column), (after.row, after.column));
}

#[test]
fn a_hint_is_no_longer_than_the_panel_may_make_it() {
    let clipped = Hint::new("far too many glyphs".chars());

    assert_eq!(clipped.glyphs().count(), Hint::CAPACITY);
}
