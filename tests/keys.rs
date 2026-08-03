//! The terminal backend's mapping from keys to controls.
//!
//! The one test file that knows a keyboard exists, because it tests the one
//! layer allowed to know. Everything above is handed controls.

use std::io::{self, Read};

use motif::device::{Button, Encoder};
use motif::ui::{ControlEvent, Controls, KeyReader, Turn};

/// A source that hands over one chunk per read, standing in for keys that
/// arrive split across reads.
struct Chunks {
    remaining: Vec<Vec<u8>>,
}

impl Chunks {
    fn new(chunks: &[&[u8]]) -> Self {
        let mut remaining: Vec<Vec<u8>> = chunks.iter().map(|chunk| chunk.to_vec()).collect();
        remaining.reverse();
        Self { remaining }
    }
}

impl Read for Chunks {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let Some(chunk) = self.remaining.pop() else {
            return Ok(0);
        };
        buffer[..chunk.len()].copy_from_slice(&chunk);
        Ok(chunk.len())
    }
}

/// A source that refuses every read, standing in for a keyboard that has gone.
struct BrokenSource;

impl Read for BrokenSource {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the keyboard is gone"))
    }
}

fn events_from(source: impl Read) -> Vec<ControlEvent> {
    let mut reader = KeyReader::new(source);
    std::iter::from_fn(|| reader.poll()).collect()
}

fn events(bytes: &[u8]) -> Vec<ControlEvent> {
    events_from(bytes)
}

fn first(bytes: &[u8]) -> Option<ControlEvent> {
    events(bytes).into_iter().next()
}

fn pressed(button: Button) -> Option<ControlEvent> {
    Some(ControlEvent::Pressed {
        button,
        shifted: false,
    })
}

fn turned(encoder: Encoder, turn: Turn) -> Option<ControlEvent> {
    Some(ControlEvent::Turned {
        encoder,
        turn,
        shifted: false,
    })
}

#[test]
fn the_right_key_of_a_pair_turns_its_encoder_clockwise() {
    assert_eq!(first(b"w"), turned(Encoder::First, Turn::Clockwise));
}

#[test]
fn the_left_key_of_a_pair_turns_it_the_other_way() {
    assert_eq!(first(b"q"), turned(Encoder::First, Turn::Anticlockwise));
}

#[test]
fn every_encoder_has_a_pair_of_its_own() {
    assert_eq!(first(b"e"), turned(Encoder::Second, Turn::Anticlockwise));
    assert_eq!(first(b"r"), turned(Encoder::Second, Turn::Clockwise));
    assert_eq!(first(b"t"), turned(Encoder::Third, Turn::Anticlockwise));
    assert_eq!(first(b"y"), turned(Encoder::Third, Turn::Clockwise));
    assert_eq!(first(b"u"), turned(Encoder::Fourth, Turn::Anticlockwise));
    assert_eq!(first(b"i"), turned(Encoder::Fourth, Turn::Clockwise));
}

#[test]
fn an_arrow_key_presses_the_button_it_points_at() {
    assert_eq!(first(b"\x1b[A"), pressed(Button::Up));
    assert_eq!(first(b"\x1b[B"), pressed(Button::Down));
    assert_eq!(first(b"\x1b[C"), pressed(Button::Right));
    assert_eq!(first(b"\x1b[D"), pressed(Button::Left));
}

#[test]
fn an_arrow_in_application_mode_presses_the_same_button() {
    assert_eq!(first(b"\x1bOA"), pressed(Button::Up));
}

#[test]
fn a_transport_key_presses_its_button() {
    assert_eq!(first(b"z"), pressed(Button::Play));
    assert_eq!(first(b"x"), pressed(Button::Stop));
    assert_eq!(first(b"c"), pressed(Button::Record));
}

#[test]
fn an_upper_case_key_is_the_same_control_shifted() {
    assert_eq!(
        first(b"W"),
        Some(ControlEvent::Turned {
            encoder: Encoder::First,
            turn: Turn::Clockwise,
            shifted: true,
        })
    );
    assert_eq!(
        first(b"Z"),
        Some(ControlEvent::Pressed {
            button: Button::Play,
            shifted: true,
        })
    );
}

#[test]
fn an_arrow_held_with_shift_is_the_same_button_shifted() {
    assert_eq!(
        first(b"\x1b[1;2A"),
        Some(ControlEvent::Pressed {
            button: Button::Up,
            shifted: true,
        })
    );
}

#[test]
fn an_arrow_with_another_modifier_is_not_shifted() {
    assert_eq!(first(b"\x1b[1;3A"), pressed(Button::Up));
}

#[test]
fn a_key_that_is_not_on_the_panel_is_no_event() {
    assert!(events(b".").is_empty());
    assert!(events(b"\x1b[H").is_empty());
}

#[test]
fn keys_that_arrive_together_are_separate_events() {
    assert_eq!(
        events(b"zx"),
        vec![
            ControlEvent::Pressed {
                button: Button::Play,
                shifted: false
            },
            ControlEvent::Pressed {
                button: Button::Stop,
                shifted: false
            },
        ]
    );
}

#[test]
fn a_key_after_an_unmapped_one_is_still_read() {
    assert_eq!(first(b".z"), pressed(Button::Play));
}

#[test]
fn an_escape_sequence_split_across_reads_is_one_event() {
    assert_eq!(
        events_from(Chunks::new(&[b"\x1b[", b"A"])),
        vec![ControlEvent::Pressed {
            button: Button::Up,
            shifted: false
        }]
    );
}

#[test]
fn an_escape_that_starts_nothing_does_not_swallow_the_next_key() {
    assert_eq!(first(b"\x1bz"), pressed(Button::Play));
}

#[test]
fn an_escape_sequence_that_never_ends_is_no_event() {
    assert!(events(b"\x1b[").is_empty());
}

#[test]
fn nothing_to_read_is_no_event() {
    assert!(events(b"").is_empty());
}

#[test]
fn a_source_that_refuses_a_read_is_no_event() {
    assert!(events_from(BrokenSource).is_empty());
}
