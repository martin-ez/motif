//! The terminal's keyboard, read as the panel's controls.
//!
//! The only place in the project where a key exists. What leaves here is a
//! [`ControlEvent`], so the mapping can change, or be replaced by a firmware
//! backend reading GPIO, without anything above noticing.

use std::io::Read;

use crate::device::{Button, Control, Encoder};
use crate::ui::{ControlEvent, Controls, Hint, Turn};

const ESCAPE: u8 = 0x1b;
const INTERRUPT: u8 = 0x03;
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

enum Taken {
    Press(Press),
    Interrupt,
}

enum Step {
    Took { bytes: usize, taken: Option<Taken> },
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
        taken: arrow(parameters[length])
            .map(|key| Press {
                key,
                shifted: shift_held(&parameters[..length]),
            })
            .map(Taken::Press),
    }
}

fn single_shift(bytes: &[u8]) -> Step {
    match bytes.get(2) {
        None => Step::Incomplete,
        Some(final_byte) => Step::Took {
            bytes: 3,
            taken: arrow(*final_byte)
                .map(|key| Press {
                    key,
                    shifted: false,
                })
                .map(Taken::Press),
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
            taken: None,
        },
    }
}

fn typed(byte: u8) -> Step {
    if byte == INTERRUPT {
        return Step::Took {
            bytes: 1,
            taken: Some(Taken::Interrupt),
        };
    }

    let typed = char::from(byte);
    let taken = typed
        .is_ascii_graphic()
        .then(|| Press {
            key: Key::Glyph(typed.to_ascii_lowercase()),
            shifted: typed.is_ascii_uppercase(),
        })
        .map(Taken::Press);

    Step::Took { bytes: 1, taken }
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
/// Scenes are `1`–`4`; transport `z`, `x`, `c` for play, stop and record;
/// navigation the arrow keys; the encoder turns with `,` and `.`. Shift is an
/// upper case letter or an arrow carrying modifier 2, resolved here rather than
/// reported as a press; the same mapping names a control for the picture. Ctrl+C
/// reaches no control and interrupts the panel instead.
///
/// Reads are never waited on and a poll gives up after a bufferful of bytes
/// yielding no control. A split key waits for the rest; a stray byte is dropped.
pub struct KeyReader<R: Read> {
    source: R,
    pending: [u8; PENDING_CAPACITY],
    filled: usize,
    interrupted: bool,
}

impl<R: Read> KeyReader<R> {
    /// A reader taking keys from `source`.
    pub fn new(source: R) -> Self {
        Self {
            source,
            pending: [0; PENDING_CAPACITY],
            filled: 0,
            interrupted: false,
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

    fn interrupted(&self) -> bool {
        self.interrupted
    }

    fn poll(&mut self) -> Option<ControlEvent> {
        for _ in 0..PENDING_CAPACITY {
            if self.interrupted {
                return None;
            }

            match next_press(&self.pending[..self.filled]) {
                Step::Took { bytes, taken } => {
                    self.take(bytes);
                    match taken {
                        Some(Taken::Interrupt) => self.interrupted = true,
                        Some(Taken::Press(press)) => {
                            if let Some(event) = control_of(press) {
                                return Some(event);
                            }
                        }
                        None => {}
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
