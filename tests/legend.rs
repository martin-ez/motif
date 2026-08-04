//! What a page says its controls do, and how that reaches the screen.
//!
//! The legend is where a page's meanings and a backend's glyphs meet, so this
//! is the one thing that has to hold both without either knowing the other. No
//! key is named here: the panels below hand out glyphs of their own invention,
//! which is all a legend ever learns about how a control is reached.

use motif::device::{Button, Control, DeviceProfile, Encoder, ScreenProfile};
use motif::ui::{ControlEvent, Controls, Frame, Hint, Legend};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;

/// The glyph the panels below reach `control` by.
///
/// Upper case, because every meaning in this file is lower case: a test that a
/// panel drew no glyphs is only worth anything if a glyph cannot hide inside a
/// meaning.
fn glyph_of(control: Control) -> char {
    let position = Control::ALL
        .iter()
        .position(|listed| *listed == control)
        .expect("every control is listed in ALL");

    char::from(b'A' + position as u8)
}

fn named(control: Control, meaning: &str) -> String {
    format!("{} {meaning}", glyph_of(control))
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

fn place_of(frame: &Frame, glyph: char) -> Option<(usize, usize)> {
    (0..SCREEN.rows).find_map(|row| {
        let column = row_of(frame, row)
            .chars()
            .position(|drawn| drawn == glyph)?;
        Some((row, column))
    })
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
fn a_control_is_named_by_the_glyph_that_reaches_it() {
    let frame = drawn(&Legend::blank().naming(Button::Play, "play"), &Lettered);

    assert!(text_of(&frame).contains(&named(Control::Button(Button::Play), "play")));
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

    assert!(text_of(&frame).contains(&named(Control::Button(Button::Up), "-")));
}

#[test]
fn an_encoder_is_on_screen_beside_the_buttons() {
    let frame = drawn(&Legend::blank().naming(Encoder::Third, "gain"), &Lettered);

    assert!(text_of(&frame).contains(&named(Control::Encoder(Encoder::Third), "gain")));
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
fn an_entry_keeps_its_place_whatever_the_page_means_by_it() {
    let bare = drawn(&Legend::blank(), &Lettered);
    let filled = drawn(
        &Legend::blank()
            .naming(Button::Play, "play")
            .naming(Encoder::First, "gain"),
        &Lettered,
    );
    let last = glyph_of(Control::Encoder(Encoder::Fourth));

    assert!(place_of(&bare, last).is_some());
    assert_eq!(place_of(&bare, last), place_of(&filled, last));
}

#[test]
fn a_meaning_too_long_for_its_entry_does_not_reach_the_next_one() {
    let crowded = drawn(
        &Legend::blank().naming(Button::Up, "a meaning far longer than one entry holds"),
        &Lettered,
    );
    let bare = drawn(&Legend::blank(), &Lettered);
    let next = glyph_of(Control::Button(Button::Down));

    assert!(place_of(&bare, next).is_some());
    assert_eq!(place_of(&crowded, next), place_of(&bare, next));
}

#[test]
fn a_hint_is_no_longer_than_the_panel_may_make_it() {
    let clipped = Hint::new("far too many glyphs".chars());

    assert_eq!(clipped.glyphs().count(), Hint::CAPACITY);
}
