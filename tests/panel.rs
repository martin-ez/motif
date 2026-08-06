//! The panel a screen draws for a device that has none: which key reaches what.
//!
//! No key is named here: the panels below hand out glyphs of their own
//! invention, which is all the picture ever learns about how a control is
//! reached.
//!
//! The drawing is checked by where things land rather than by the text of a
//! row: a key is an edge with a glyph inside it, and the arrangement of those
//! keys is the picture of the panel.

use motif::device::{Button, Control, Encoder};
use motif::ui::{ControlEvent, Controls, Hint, Marks, Panel, Turn};

const LIGHT: char = '─';
const LIGHT_WALL: char = '│';
const HEAVY: char = '━';
const HEAVY_WALL: char = '┃';
const ROUND_TOP_LEFT: char = '╭';
const ROUND_TOP_RIGHT: char = '╮';

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

/// A panel that names each control with three glyphs, as a terminal names the
/// encoder by the pair of keys that turn it.
struct Wordy;

impl Controls for Wordy {
    fn poll(&mut self) -> Option<ControlEvent> {
        None
    }

    fn hint(&self, control: Control) -> Option<Hint> {
        Some(Hint::new(['(', glyph_of(control), ')']))
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

fn drawn(panel: &impl Controls) -> Panel {
    Panel::showing(panel, Marks::none())
}

fn drawn_pressing(panel: &impl Controls, button: Button) -> Panel {
    let mut marks = Marks::none();
    marks.fired(ControlEvent::Pressed {
        button,
        shifted: false,
    });

    Panel::showing(panel, marks)
}

fn drawn_turning(panel: &impl Controls, turn: Turn) -> Panel {
    let mut marks = Marks::none();
    marks.fired(ControlEvent::Turned {
        encoder: Encoder::Main,
        turn,
        shifted: false,
    });

    Panel::showing(panel, marks)
}

fn row_of(panel: &Panel, row: usize) -> String {
    (0..Panel::COLUMNS)
        .filter_map(|column| panel.get(column, row))
        .map(|cell| cell.glyph())
        .collect()
}

fn text_of(panel: &Panel) -> String {
    (0..Panel::ROWS)
        .map(|row| row_of(panel, row))
        .collect::<Vec<_>>()
        .join("\n")
}

fn glyph_at(panel: &Panel, column: usize, row: usize) -> char {
    panel
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
fn found(panel: &Panel, text: &str) -> At {
    (0..Panel::ROWS)
        .find_map(|row| {
            let drawn = row_of(panel, row);
            let at = drawn.find(text)?;
            Some(At {
                row,
                column: drawn[..at].chars().count(),
            })
        })
        .unwrap_or_else(|| panic!("{text:?} is nowhere on the panel"))
}

fn key_of(panel: &Panel, control: impl Into<Control>) -> At {
    found(panel, &String::from(glyph_of(control)))
}

#[test]
fn every_key_wears_the_glyph_that_reaches_it() {
    let panel = drawn(&Lettered);
    let text = text_of(&panel);

    for control in Control::ALL {
        assert!(
            text.contains(glyph_of(control)),
            "{control:?} is on the panel and unnamed in the picture"
        );
    }
}

#[test]
fn nothing_but_the_glyph_is_written_on_a_key() {
    let panel = drawn(&Lettered);
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column - 1, key.row), ' ');
    assert_eq!(glyph_at(&panel, key.column + 1, key.row), ' ');
}

#[test]
fn a_key_at_rest_is_drawn_light_all_round() {
    let panel = drawn(&Lettered);

    for button in [Button::Play, Button::Stop] {
        let key = key_of(&panel, button);

        assert_eq!(glyph_at(&panel, key.column, key.row - 1), LIGHT);
        assert_eq!(glyph_at(&panel, key.column, key.row + 1), LIGHT);
        assert_eq!(glyph_at(&panel, key.column - 2, key.row), LIGHT_WALL);
        assert_eq!(glyph_at(&panel, key.column + 2, key.row), LIGHT_WALL);
    }
}

#[test]
fn a_button_whose_event_was_delivered_is_drawn_heavy_all_round() {
    let panel = drawn_pressing(&Lettered, Button::Play);
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), HEAVY);
    assert_eq!(glyph_at(&panel, key.column, key.row + 1), HEAVY);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), HEAVY_WALL);
    assert_eq!(glyph_at(&panel, key.column + 2, key.row), HEAVY_WALL);
}

#[test]
fn a_button_that_did_not_fire_stays_light_beside_one_that_did() {
    let panel = drawn_pressing(&Lettered, Button::Stop);
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), LIGHT);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), LIGHT_WALL);
}

#[test]
fn a_key_drawn_heavy_still_shows_the_glyph_that_reaches_it() {
    let panel = drawn_pressing(&Lettered, Button::Stop);
    let key = key_of(&panel, Button::Stop);

    assert_eq!(
        glyph_at(&panel, key.column, key.row),
        glyph_of(Button::Stop)
    );
}

#[test]
fn no_key_is_drawn_heavy_while_every_control_rests() {
    let panel = drawn(&Lettered);

    assert!(!text_of(&panel).contains(HEAVY));
    assert!(!text_of(&panel).contains(HEAVY_WALL));
}

#[test]
fn an_encoder_turned_clockwise_goes_heavy_down_its_right_side_only() {
    let panel = drawn_turning(&Lettered, Turn::Clockwise);
    let key = key_of(&panel, Encoder::Main);

    assert_eq!(glyph_at(&panel, key.column + 3, key.row), HEAVY_WALL);
    assert_eq!(glyph_at(&panel, key.column - 3, key.row), LIGHT_WALL);
}

#[test]
fn an_encoder_turned_anticlockwise_goes_heavy_down_its_left_side_only() {
    let panel = drawn_turning(&Lettered, Turn::Anticlockwise);
    let key = key_of(&panel, Encoder::Main);

    assert_eq!(glyph_at(&panel, key.column - 3, key.row), HEAVY_WALL);
    assert_eq!(glyph_at(&panel, key.column + 3, key.row), LIGHT_WALL);
}

#[test]
fn a_turned_encoder_keeps_the_light_top_and_bottom_it_rests_with() {
    let panel = drawn_turning(&Lettered, Turn::Clockwise);
    let key = key_of(&panel, Encoder::Main);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), LIGHT);
    assert_eq!(glyph_at(&panel, key.column, key.row + 1), LIGHT);
}

#[test]
fn an_encoder_at_rest_is_rounded_on_both_sides() {
    let panel = drawn(&Lettered);
    let key = key_of(&panel, Encoder::Main);

    assert_eq!(
        glyph_at(&panel, key.column - 3, key.row - 1),
        ROUND_TOP_LEFT
    );
    assert_eq!(
        glyph_at(&panel, key.column + 3, key.row - 1),
        ROUND_TOP_RIGHT
    );
}

#[test]
fn a_turned_encoder_rounds_the_corner_it_did_not_move_towards() {
    let panel = drawn_turning(&Lettered, Turn::Clockwise);
    let key = key_of(&panel, Encoder::Main);

    assert_eq!(
        glyph_at(&panel, key.column - 3, key.row - 1),
        ROUND_TOP_LEFT
    );
    assert_ne!(
        glyph_at(&panel, key.column + 3, key.row - 1),
        ROUND_TOP_RIGHT
    );
}

#[test]
fn a_navigation_key_shows_the_arrow_that_reaches_it() {
    let panel = drawn(&Lettered);

    for arrow in [Button::Up, Button::Down, Button::Left, Button::Right] {
        let key = key_of(&panel, arrow);

        assert_eq!(glyph_at(&panel, key.column, key.row), glyph_of(arrow));
    }
}

#[test]
fn a_panel_that_labels_its_own_keys_is_still_drawn_without_them() {
    let panel = drawn(&Unlabelled);
    let text = text_of(&panel);

    assert!(text.contains(LIGHT));
    for control in Control::ALL {
        assert!(
            !text.contains(glyph_of(control)),
            "{control:?} was named by a glyph the panel does not have"
        );
    }
}

#[test]
fn the_navigation_keys_are_drawn_in_the_pattern_they_sit_in() {
    let panel = drawn(&Lettered);
    let (up, down) = (key_of(&panel, Button::Up), key_of(&panel, Button::Down));
    let (left, right) = (key_of(&panel, Button::Left), key_of(&panel, Button::Right));

    assert_eq!(up.column, down.column);
    assert!(up.row < down.row);
    assert_eq!(left.row, down.row);
    assert_eq!(right.row, down.row);
    assert!(left.column < down.column);
    assert!(down.column < right.column);
}

#[test]
fn the_scene_buttons_run_left_to_right_in_a_row_of_their_own() {
    let panel = drawn(&Lettered);
    let scenes = [
        Button::FirstScene,
        Button::SecondScene,
        Button::ThirdScene,
        Button::FourthScene,
    ]
    .map(|scene| key_of(&panel, scene));

    for pair in scenes.windows(2) {
        assert_eq!(pair[0].row, pair[1].row);
        assert!(pair[0].column < pair[1].column);
    }
}

#[test]
fn the_action_keys_are_drawn_under_the_scene_buttons() {
    let panel = drawn(&Lettered);
    let scene = key_of(&panel, Button::FirstScene);
    let actions = [Button::Play, Button::Stop, Button::Record].map(|action| key_of(&panel, action));

    assert_eq!(actions[0].column, scene.column);
    for action in &actions {
        assert_eq!(action.row, scene.row + 3);
    }
}

#[test]
fn shift_is_a_key_on_the_panel_like_any_other() {
    let panel = drawn(&Lettered);
    let shift = key_of(&panel, Button::Shift);
    let record = key_of(&panel, Button::Record);

    assert_eq!(shift.row, record.row);
    assert!(shift.column > record.column);
}

#[test]
fn the_encoder_is_drawn_as_a_knob_rather_than_a_key() {
    let panel = drawn(&Lettered);
    let knob = key_of(&panel, Encoder::Main);
    let (opens, closes) = (found(&panel, "╭"), found(&panel, "╯"));

    assert_eq!(opens.row, knob.row - 1);
    assert!(opens.column < knob.column);
    assert_eq!(closes.row, knob.row + 1);
    assert!(closes.column > knob.column);
}

#[test]
fn a_hint_of_several_glyphs_is_centred_on_its_key() {
    let panel = drawn(&Wordy);
    let opens = found(&panel, "╭");
    let closes = found(&panel, "╮");
    let face: String = (opens.column..=closes.column)
        .map(|column| glyph_at(&panel, column, opens.row + 1))
        .collect();

    assert_eq!(face, format!("│ ({}) │", glyph_of(Encoder::Main)));
}

#[test]
fn the_cross_keys_are_evenly_spaced() {
    let panel = drawn(&Lettered);
    let (left, down) = (key_of(&panel, Button::Left), key_of(&panel, Button::Down));
    let right = key_of(&panel, Button::Right);

    assert_eq!(down.column - left.column, right.column - down.column);
}

#[test]
fn the_picture_is_no_larger_than_the_keys_drawn_on_it() {
    let panel = drawn(&Lettered);
    let drawn_columns: Vec<usize> = (0..Panel::COLUMNS)
        .filter(|column| (0..Panel::ROWS).any(|row| glyph_at(&panel, *column, row) != ' '))
        .collect();

    assert_eq!(drawn_columns.first(), Some(&0));
    assert_eq!(drawn_columns.last(), Some(&(Panel::COLUMNS - 1)));
    assert_ne!(row_of(&panel, 0).trim(), "");
    assert_ne!(row_of(&panel, Panel::ROWS - 1).trim(), "");
}

#[test]
fn a_blank_panel_is_the_size_of_the_picture_and_empty() {
    let blank = Panel::blank();

    assert_eq!(blank.cells().len(), Panel::COLUMNS * Panel::ROWS);
    assert_eq!(text_of(&blank).trim(), "");
}

#[test]
fn nothing_is_drawn_past_the_edge_of_the_picture() {
    let panel = drawn(&Lettered);

    assert_eq!(panel.get(Panel::COLUMNS, 0), None);
    assert_eq!(panel.get(0, Panel::ROWS), None);
}

#[test]
fn a_hint_is_no_longer_than_the_panel_may_make_it() {
    let clipped = Hint::new("far too many glyphs".chars());

    assert_eq!(clipped.glyphs().count(), Hint::CAPACITY);
}
