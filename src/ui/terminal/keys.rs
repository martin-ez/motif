//! The terminal's keyboard, read as the panel's controls.
//!
//! The only place in the project where a key exists. What leaves here is a
//! [`ControlEvent`], so the mapping can change, or be replaced by a firmware
//! backend reading GPIO, without anything above noticing.

use std::io::Read;

use crate::device::{Button, Control, Encoder};
use crate::ui::{ControlEvent, Controls, Hint, Turn};

const ESCAPE: u8 = 0x1b;
const SHIFT: char = '⇧';
const TURN: char = '/';
const PARAMETERS_START: usize = 2;
const PENDING_CAPACITY: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Glyph(char),
    Up,
    Down,
    Left,
    Right,
    Shift,
}

#[derive(Clone, Copy)]
struct Press {
    key: Key,
    shifted: bool,
}

enum Step {
    Took { bytes: usize, press: Option<Press> },
    Incomplete,
}

fn key_of(button: Button) -> Key {
    match button {
        Button::Up => Key::Up,
        Button::Down => Key::Down,
        Button::Left => Key::Left,
        Button::Right => Key::Right,
        Button::FirstScene => Key::Glyph('1'),
        Button::SecondScene => Key::Glyph('2'),
        Button::ThirdScene => Key::Glyph('3'),
        Button::FourthScene => Key::Glyph('4'),
        Button::Play => Key::Glyph('z'),
        Button::Stop => Key::Glyph('x'),
        Button::Record => Key::Glyph('c'),
        Button::Shift => Key::Shift,
    }
}

fn keys_of(encoder: Encoder) -> [Key; 2] {
    match encoder {
        Encoder::Main => [Key::Glyph(','), Key::Glyph('.')],
    }
}

fn button_pressed(press: Press) -> Option<ControlEvent> {
    let button = Button::ALL
        .into_iter()
        .find(|button| key_of(*button) == press.key)?;

    Some(ControlEvent::Pressed {
        button,
        shifted: press.shifted,
    })
}

fn encoder_turned(press: Press) -> Option<ControlEvent> {
    Encoder::ALL.into_iter().find_map(|encoder| {
        let [anticlockwise, clockwise] = keys_of(encoder);
        let turn = if press.key == anticlockwise {
            Turn::Anticlockwise
        } else if press.key == clockwise {
            Turn::Clockwise
        } else {
            return None;
        };

        Some(ControlEvent::Turned {
            encoder,
            turn,
            shifted: press.shifted,
        })
    })
}

fn control_of(press: Press) -> Option<ControlEvent> {
    button_pressed(press).or_else(|| encoder_turned(press))
}

fn glyph_of(key: Key) -> char {
    match key {
        Key::Glyph(glyph) => glyph,
        Key::Up => '^',
        Key::Down => 'v',
        Key::Left => '<',
        Key::Right => '>',
        Key::Shift => SHIFT,
    }
}

fn hint_of(control: Control) -> Hint {
    match control {
        Control::Button(button) => Hint::new([glyph_of(key_of(button))]),
        Control::Encoder(encoder) => {
            let [anticlockwise, clockwise] = keys_of(encoder);
            Hint::new([glyph_of(anticlockwise), TURN, glyph_of(clockwise)])
        }
    }
}

fn arrow(final_byte: u8) -> Option<Key> {
    match final_byte {
        b'A' => Some(Key::Up),
        b'B' => Some(Key::Down),
        b'C' => Some(Key::Right),
        b'D' => Some(Key::Left),
        _ => None,
    }
}

fn is_parameter(byte: u8) -> bool {
    (0x30..=0x3f).contains(&byte)
}

fn shift_held(parameters: &[u8]) -> bool {
    parameters
        .split(|byte| *byte == b';')
        .nth(1)
        .and_then(|modifier| std::str::from_utf8(modifier).ok())
        .and_then(|modifier| modifier.parse::<u8>().ok())
        .is_some_and(|modifier| modifier.saturating_sub(1) & 1 != 0)
}

fn control_sequence(bytes: &[u8]) -> Step {
    let parameters = &bytes[PARAMETERS_START..];
    let Some(length) = parameters.iter().position(|byte| !is_parameter(*byte)) else {
        return Step::Incomplete;
    };

    Step::Took {
        bytes: PARAMETERS_START + length + 1,
        press: arrow(parameters[length]).map(|key| Press {
            key,
            shifted: shift_held(&parameters[..length]),
        }),
    }
}

fn single_shift(bytes: &[u8]) -> Step {
    match bytes.get(2) {
        None => Step::Incomplete,
        Some(final_byte) => Step::Took {
            bytes: 3,
            press: arrow(*final_byte).map(|key| Press {
                key,
                shifted: false,
            }),
        },
    }
}

fn escaped(bytes: &[u8]) -> Step {
    match bytes.get(1) {
        None => Step::Incomplete,
        Some(b'[') => control_sequence(bytes),
        Some(b'O') => single_shift(bytes),
        Some(_) => Step::Took {
            bytes: 1,
            press: None,
        },
    }
}

fn typed(byte: u8) -> Step {
    let typed = char::from(byte);
    let press = typed.is_ascii_graphic().then(|| Press {
        key: Key::Glyph(typed.to_ascii_lowercase()),
        shifted: typed.is_ascii_uppercase(),
    });

    Step::Took { bytes: 1, press }
}

fn next_press(bytes: &[u8]) -> Step {
    match bytes.first() {
        None => Step::Incomplete,
        Some(&ESCAPE) => escaped(bytes),
        Some(byte) => typed(*byte),
    }
}

/// The keys a terminal sends, reported as the panel's controls.
///
/// The scene buttons are the number row, `1` to `4`, left to right in panel
/// order. Transport sits on `z`, `x` and `c`, in the panel's order of play,
/// stop and record; navigation is on the arrow keys; the encoder turns with
/// `,` and `.`, which sit under the right hand beside them, anticlockwise on
/// the left. Holding shift is an upper case letter, or an arrow whose escape
/// sequence carries modifier 2, and it is resolved here rather than reported as
/// a control of its own. A shifted digit is whatever glyph the player's layout
/// puts there, so it reaches nothing.
///
/// Shift is the one button on the panel that never leaves here as a press. A
/// terminal does not report the key at all — it reports what was typed while it
/// was held — so there is nothing to send until the control it modifies
/// arrives, and then it is that control's event carrying it.
///
/// The same mapping is what the reader hands the screen to name a control by,
/// so the legend a player reads cannot disagree with the keys that work. A key
/// that has no glyph of its own is named by the shape it points in — `^`, `v`,
/// `<`, `>`, `⇧` — and the encoder by its pair, as `,/.`.
///
/// Reads are never waited on: a read that hands back nothing ends the poll, so
/// a source that blocks until a key is pressed will spend the frame budget. The
/// terminal is put into a mode that returns immediately by
/// [`TerminalScreen`](super::TerminalScreen).
///
/// A poll is bounded work for the same reason. It gives up after a bufferful of
/// bytes that yield no control, so a source that never runs dry — a pasted page
/// of text arriving as keystrokes — costs one frame rather than hanging in one,
/// and what it did not reach is still there for the next poll.
///
/// A key that arrives split across reads — an escape sequence is several bytes,
/// and a terminal is under no obligation to deliver them together — is held
/// until the rest of it turns up. A byte that begins nothing the panel has is
/// dropped, and the bytes after it are still read.
pub struct KeyReader<R: Read> {
    source: R,
    pending: [u8; PENDING_CAPACITY],
    filled: usize,
}

impl<R: Read> KeyReader<R> {
    /// A reader taking keys from `source`.
    pub fn new(source: R) -> Self {
        Self {
            source,
            pending: [0; PENDING_CAPACITY],
            filled: 0,
        }
    }

    fn take(&mut self, bytes: usize) {
        self.pending.copy_within(bytes..self.filled, 0);
        self.filled -= bytes;
    }

    fn fill(&mut self) -> bool {
        if self.filled == self.pending.len() {
            self.filled = 0;
        }

        match self.source.read(&mut self.pending[self.filled..]) {
            Ok(0) | Err(_) => false,
            Ok(read) => {
                self.filled += read;
                true
            }
        }
    }
}

impl<R: Read> Controls for KeyReader<R> {
    fn hint(&self, control: Control) -> Option<Hint> {
        Some(hint_of(control))
    }

    fn poll(&mut self) -> Option<ControlEvent> {
        for _ in 0..PENDING_CAPACITY {
            match next_press(&self.pending[..self.filled]) {
                Step::Took { bytes, press } => {
                    self.take(bytes);
                    if let Some(event) = press.and_then(control_of) {
                        return Some(event);
                    }
                }
                Step::Incomplete => {
                    if !self.fill() {
                        return None;
                    }
                }
            }
        }

        None
    }
}
