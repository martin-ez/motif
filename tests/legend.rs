//! What a page says its controls do, and how that reaches the screen.
//!
//! The legend is where a page's meanings and a backend's glyphs meet, so this
//! is the one thing that has to hold both without either knowing the other. No
//! key is named here: the panels below hand out glyphs of their own invention,
//! which is all a legend ever learns about how a control is reached.
//!
//! The drawing is checked by where things land rather than by the text of a
//! row: a key is a border with a glyph inside it and its meaning beneath, and
//! that is what the assertions look for.

use motif::device::{Button, Control, DeviceProfile, Encoder, ScreenProfile};
use motif::ui::{ControlEvent, Controls, Frame, Hint, Legend};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;

/// The glyph the panels below reach `control` by.
///
/// Upper case, because every meaning in this file is lower case: a test that a
/// panel drew no glyphs is only worth anything if a glyph cannot hide inside a
/// meaning.
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

/// A panel whose controls are labelled on the hardware, so the screen has
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

/// Where `text` was drawn, counted in cells rather than bytes: a row of keys is
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
fn a_page_declares_what_a_control_does_on_it() {
    let legend = Legend::blank().naming(Button::Play, "play");

    assert_eq!(legend.meaning(Button::Play), Some("play"));
}

#[test]
fn a_control_a_page_leaves_alone_means_nothing_on_it() {
    let legend = Legend::blank().naming(Button::Play, "play");

    assert_eq!(legend.meaning(Button::Up), None);
    assert_eq!(legend.meaning(Encoder::First), None);
}

#[test]
fn declaring_a_control_twice_keeps_the_later_meaning() {
    let legend = Legend::blank()
        .naming(Encoder::First, "gain")
        .naming(Encoder::First, "level");

    assert_eq!(legend.meaning(Encoder::First), Some("level"));
}

#[test]
fn what_a_control_does_is_on_screen() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Lettered);

    assert!(text_of(&frame).contains("play"));
}

#[test]
fn a_control_is_drawn_as_a_key_with_an_edge_around_it() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Lettered);
    let key = key_of(&frame, Button::Play);

    assert_eq!(glyph_at(&frame, key.column, key.row - 1), '─');
    assert_eq!(glyph_at(&frame, key.column, key.row + 1), '─');
    assert_eq!(glyph_at(&frame, key.column - 2, key.row), '│');
    assert_eq!(glyph_at(&frame, key.column + 2, key.row), '│');
}

#[test]
fn what_a_control_does_is_written_under_its_key() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Lettered);
    let key = key_of(&frame, Button::Play);
    let meaning = found(&frame, "play");

    assert_eq!(meaning.row, key.row + 2);
    assert!(meaning.column.abs_diff(key.column) <= 2);
}

#[test]
fn an_encoder_is_drawn_as_a_key_of_its_own() {
    let frame = drawn(&Legend::blank().naming(Encoder::Third, "gain"), &Lettered);
    let key = key_of(&frame, Encoder::Third);
    let meaning = found(&frame, "gain");

    assert_eq!(glyph_at(&frame, key.column, key.row - 1), '─');
    assert_eq!(meaning.row, key.row + 2);
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
fn the_key_at_the_top_of_the_cluster_is_named_beside_it() {
    let frame = drawn(&Legend::blank().naming(Button::Up, "up"), &Lettered);
    let key = key_of(&frame, Button::Up);
    let meaning = found(&frame, "up");

    assert_eq!(meaning.row, key.row);
    assert!(meaning.column > key.column);
}

#[test]
fn every_control_on_the_panel_is_on_screen() {
    let frame = drawn(&Legend::blank(), &Lettered);
    let text = text_of(&frame);

    for control in Control::ALL {
        assert!(
            text.contains(glyph_of(control)),
            "{control:?} is not on the screen"
        );
    }
}

#[test]
fn a_control_the_page_does_not_answer_reads_as_unavailable() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Lettered);
    let key = key_of(&frame, Button::Stop);

    assert_eq!(glyph_at(&frame, key.column, key.row + 2), '-');
}

#[test]
fn a_panel_that_labels_its_own_controls_is_drawn_without_glyphs() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Unlabelled);
    let text = text_of(&frame);

    assert!(text.contains("play"));
    for control in Control::ALL {
        assert!(
            !text.contains(glyph_of(control)),
            "{control:?} was named by a glyph the panel does not have"
        );
    }
}

#[test]
fn the_legend_keeps_to_the_rows_it_reserves() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Lettered);

    for row in 0..SCREEN.rows - Legend::ROWS {
        assert_eq!(row_of(&frame, row).trim(), "", "row {row} was drawn on");
    }
}

#[test]
fn a_key_keeps_its_place_whatever_the_page_means_by_it() {
    let bare = drawn(&Legend::blank(), &Lettered);
    let filled = drawn(
        &Legend::blank()
            .naming(Button::Play, "play")
            .naming(Encoder::First, "gain"),
        &Lettered,
    );
    let (before, after) = (
        key_of(&bare, Encoder::Fourth),
        key_of(&filled, Encoder::Fourth),
    );

    assert_eq!((before.row, before.column), (after.row, after.column));
}

#[test]
fn a_meaning_too_long_for_its_key_does_not_reach_the_next_one() {
    let crowded = drawn(
        &Legend::blank().naming(Encoder::First, "a meaning far longer than a key is wide"),
        &Lettered,
    );
    let next = key_of(&crowded, Encoder::Second);

    assert_eq!(glyph_at(&crowded, next.column, next.row + 2), '-');
}

#[test]
fn a_hint_is_no_longer_than_the_panel_may_make_it() {
    let clipped = Hint::new("far too many glyphs".chars());

    assert_eq!(clipped.glyphs().count(), Hint::CAPACITY);
}
