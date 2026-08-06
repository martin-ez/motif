//! Which controls a page answers, and the panel a screen draws from that.
//!
//! The legend is where a page's declaration and a backend's glyphs meet, so this
//! is the one thing that has to hold both without either knowing the other. No
//! key is named here: the panels below hand out glyphs of their own invention,
//! which is all a legend ever learns about how a control is reached.
//!
//! The drawing is checked by where things land rather than by the text of a
//! row: a key is an edge with a glyph inside it, and the arrangement of those
//! keys is the picture of the panel.

use motif::device::{Button, Control, Encoder};
use motif::ui::{ControlEvent, Controls, Hint, Legend, Marks, Panel};

const LIGHT: char = '─';
const LIGHT_WALL: char = '│';
const HEAVY: char = '━';
const HEAVY_WALL: char = '┃';
const SOLID: char = '█';

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

fn drawn(legend: &Legend, panel: &impl Controls) -> Panel {
    legend.picture(panel, Marks::none())
}

fn drawn_firing(legend: &Legend, panel: &impl Controls, control: impl Into<Control>) -> Panel {
    let mut marks = Marks::none();
    marks.fired(control);

    legend.picture(panel, marks)
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
fn a_legend_also_answering_another_answers_what_each_answers() {
    let legend = Legend::blank()
        .answering(Button::Play)
        .also_answering(Legend::blank().answering(Button::Up));

    assert!(legend.answers(Button::Play));
    assert!(legend.answers(Button::Up));
}

#[test]
fn a_control_neither_legend_answers_stays_unanswered() {
    let legend = Legend::blank()
        .answering(Button::Play)
        .also_answering(Legend::blank().answering(Button::Up));

    assert!(!legend.answers(Button::Stop));
    assert!(!legend.answers(Encoder::Main));
}

#[test]
fn a_control_both_legends_answer_stays_answered() {
    let legend = Legend::blank()
        .answering(Button::Play)
        .also_answering(Legend::blank().answering(Button::Play));

    assert!(legend.answers(Button::Play));
}

#[test]
fn also_answering_a_blank_legend_changes_nothing() {
    let legend = Legend::blank().answering(Button::Play);

    assert_eq!(legend.also_answering(Legend::blank()), legend);
}

#[test]
fn a_blank_legend_also_answering_another_becomes_it() {
    let other = Legend::blank().answering(Button::Play);

    assert_eq!(Legend::blank().also_answering(other), other);
}

#[test]
fn every_key_wears_the_glyph_that_reaches_it_answered_or_not() {
    let panel = drawn(&Legend::blank(), &Lettered);
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
    let panel = drawn(&Legend::blank().answering(Button::Play), &Lettered);
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column - 1, key.row), ' ');
    assert_eq!(glyph_at(&panel, key.column + 1, key.row), ' ');
}

#[test]
fn a_control_the_page_answers_is_drawn_with_a_heavy_edge() {
    let panel = drawn(&Legend::blank().answering(Button::Play), &Lettered);
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), HEAVY);
    assert_eq!(glyph_at(&panel, key.column, key.row + 1), HEAVY);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), HEAVY_WALL);
    assert_eq!(glyph_at(&panel, key.column + 2, key.row), HEAVY_WALL);
}

#[test]
fn a_control_the_page_does_not_answer_is_drawn_light_rather_than_dropped() {
    let panel = drawn(&Legend::blank().answering(Button::Play), &Lettered);
    let key = key_of(&panel, Button::Stop);

    assert_eq!(
        glyph_at(&panel, key.column, key.row),
        glyph_of(Button::Stop)
    );
    assert_eq!(glyph_at(&panel, key.column, key.row - 1), LIGHT);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), LIGHT_WALL);
}

#[test]
fn a_control_whose_event_was_delivered_is_drawn_solid() {
    let panel = drawn_firing(
        &Legend::blank().answering(Button::Play),
        &Lettered,
        Button::Play,
    );
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), SOLID);
    assert_eq!(glyph_at(&panel, key.column, key.row + 1), SOLID);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), SOLID);
    assert_eq!(glyph_at(&panel, key.column + 2, key.row), SOLID);
}

#[test]
fn a_control_the_page_ignores_is_drawn_solid_when_it_fires() {
    let panel = drawn_firing(
        &Legend::blank().answering(Button::Play),
        &Lettered,
        Button::Stop,
    );
    let key = key_of(&panel, Button::Stop);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), SOLID);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), SOLID);
}

#[test]
fn an_encoder_that_fires_is_drawn_solid_rather_than_rounded() {
    let panel = drawn_firing(&Legend::blank(), &Lettered, Encoder::Main);
    let key = key_of(&panel, Encoder::Main);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), SOLID);
    assert_eq!(glyph_at(&panel, key.column - 3, key.row), SOLID);
}

#[test]
fn a_control_that_did_not_fire_keeps_the_weight_it_rests_at() {
    let panel = drawn_firing(
        &Legend::blank().answering(Button::Play),
        &Lettered,
        Button::Stop,
    );
    let key = key_of(&panel, Button::Play);

    assert_eq!(glyph_at(&panel, key.column, key.row - 1), HEAVY);
    assert_eq!(glyph_at(&panel, key.column - 2, key.row), HEAVY_WALL);
}

#[test]
fn a_key_drawn_solid_still_shows_the_glyph_that_reaches_it() {
    let panel = drawn_firing(&Legend::blank(), &Lettered, Button::Stop);
    let key = key_of(&panel, Button::Stop);

    assert_eq!(
        glyph_at(&panel, key.column, key.row),
        glyph_of(Button::Stop)
    );
}

#[test]
fn no_key_is_drawn_solid_while_every_control_rests() {
    let panel = drawn(&Legend::blank().answering(Button::Play), &Lettered);

    assert!(!text_of(&panel).contains(SOLID));
}

#[test]
fn a_navigation_key_shows_its_arrow_on_a_page_that_ignores_it() {
    let panel = drawn(&Legend::blank(), &Lettered);

    for arrow in [Button::Up, Button::Down, Button::Left, Button::Right] {
        let key = key_of(&panel, arrow);

        assert_eq!(glyph_at(&panel, key.column, key.row), glyph_of(arrow));
    }
}

#[test]
fn a_panel_that_labels_its_own_keys_still_says_which_are_live() {
    let panel = drawn(&Legend::blank().answering(Button::Play), &Unlabelled);
    let text = text_of(&panel);

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
    let panel = drawn(&Legend::blank(), &Lettered);
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
    let panel = drawn(&Legend::blank(), &Lettered);
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
    let panel = drawn(&Legend::blank(), &Lettered);
    let scene = key_of(&panel, Button::FirstScene);
    let actions = [Button::Play, Button::Stop, Button::Record].map(|action| key_of(&panel, action));

    assert_eq!(actions[0].column, scene.column);
    for action in &actions {
        assert_eq!(action.row, scene.row + 3);
    }
}

#[test]
fn shift_is_a_key_on_the_panel_like_any_other() {
    let panel = drawn(&Legend::blank(), &Lettered);
    let shift = key_of(&panel, Button::Shift);
    let record = key_of(&panel, Button::Record);

    assert_eq!(shift.row, record.row);
    assert!(shift.column > record.column);
}

#[test]
fn the_encoder_is_drawn_as_a_knob_rather_than_a_key() {
    let panel = drawn(&Legend::blank(), &Lettered);
    let knob = key_of(&panel, Encoder::Main);
    let (opens, closes) = (found(&panel, "╭"), found(&panel, "╯"));

    assert_eq!(opens.row, knob.row - 1);
    assert!(opens.column < knob.column);
    assert_eq!(closes.row, knob.row + 1);
    assert!(closes.column > knob.column);
}

#[test]
fn a_knob_the_page_answers_is_drawn_doubled_rather_than_heavy() {
    let panel = drawn(&Legend::blank().answering(Encoder::Main), &Lettered);
    let knob = key_of(&panel, Encoder::Main);

    assert_eq!(glyph_at(&panel, knob.column, knob.row - 1), '═');
    assert_eq!(glyph_at(&panel, knob.column, knob.row + 1), '═');
}

#[test]
fn a_hint_of_several_glyphs_is_centred_on_its_key() {
    let panel = drawn(&Legend::blank(), &Wordy);
    let opens = found(&panel, "╭");
    let closes = found(&panel, "╮");
    let face: String = (opens.column..=closes.column)
        .map(|column| glyph_at(&panel, column, opens.row + 1))
        .collect();

    assert_eq!(face, format!("│ ({}) │", glyph_of(Encoder::Main)));
}

#[test]
fn the_cross_keys_are_evenly_spaced() {
    let panel = drawn(&Legend::blank(), &Lettered);
    let (left, down) = (key_of(&panel, Button::Left), key_of(&panel, Button::Down));
    let right = key_of(&panel, Button::Right);

    assert_eq!(down.column - left.column, right.column - down.column);
}

#[test]
fn the_picture_is_no_larger_than_the_keys_drawn_on_it() {
    let panel = drawn(&Legend::blank(), &Lettered);
    let drawn_columns: Vec<usize> = (0..Panel::COLUMNS)
        .filter(|column| (0..Panel::ROWS).any(|row| glyph_at(&panel, *column, row) != ' '))
        .collect();

    assert_eq!(drawn_columns.first(), Some(&0));
    assert_eq!(drawn_columns.last(), Some(&(Panel::COLUMNS - 1)));
    assert_ne!(row_of(&panel, 0).trim(), "");
    assert_ne!(row_of(&panel, Panel::ROWS - 1).trim(), "");
}

#[test]
fn a_key_keeps_its_place_whether_it_is_live_or_dead() {
    let all = drawn(&Legend::blank(), &Lettered);
    let one = drawn(&Legend::blank().answering(Button::Record), &Lettered);
    let (before, after) = (key_of(&all, Button::Record), key_of(&one, Button::Record));

    assert_eq!((before.row, before.column), (after.row, after.column));
}

#[test]
fn a_blank_panel_is_the_size_of_the_picture_and_empty() {
    let blank = Panel::blank();

    assert_eq!(blank.cells().len(), Panel::COLUMNS * Panel::ROWS);
    assert_eq!(text_of(&blank).trim(), "");
}

#[test]
fn nothing_is_drawn_past_the_edge_of_the_picture() {
    let panel = drawn(&Legend::blank(), &Lettered);

    assert_eq!(panel.get(Panel::COLUMNS, 0), None);
    assert_eq!(panel.get(0, Panel::ROWS), None);
}

#[test]
fn a_hint_is_no_longer_than_the_panel_may_make_it() {
    let clipped = Hint::new("far too many glyphs".chars());

    assert_eq!(clipped.glyphs().count(), Hint::CAPACITY);
}
