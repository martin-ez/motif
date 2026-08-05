//! The panel as the screen draws it: which controls do something here.
//!
//! Two halves meet here and neither knows the other. A page declares which
//! controls it answers; a backend says what to call the way each one is
//! reached, in glyphs that belong to the panel. The legend puts the two
//! together, which is what keeps a key out of every page and a page out of
//! every backend.
//!
//! What it draws is a picture of the panel — the navigation cross the player's
//! thumb sits in, the scene buttons with the transport under them, the encoder
//! beside — and no words at all. A key the page answers wears the glyph that
//! reaches it; a key it ignores wears a dot. That is deliberately all it says:
//! a screen that explains its controls in prose can go on being unreadable,
//! where one that says only *which* controls are live has to make the rest
//! obvious where the player is already looking.

use crate::device::{Button, Control, DeviceProfile, Encoder};
use crate::ui::{Cell, Controls, Frame, Hint};

const DEAD: char = '·';
const LIVE: char = '▒';

const TOP_LEFT: char = '┌';
const TOP_RIGHT: char = '┐';
const BOTTOM_LEFT: char = '└';
const BOTTOM_RIGHT: char = '┘';
const JOIN_LEFT: char = '├';
const JOIN_RIGHT: char = '┤';
const JOIN_DOWN: char = '┬';
const JOIN_UP: char = '┴';
const CROSSING: char = '┼';
const HORIZONTAL: char = '─';
const VERTICAL: char = '│';
const ROUND_TOP_LEFT: char = '╭';
const ROUND_TOP_RIGHT: char = '╮';
const ROUND_BOTTOM_LEFT: char = '╰';
const ROUND_BOTTOM_RIGHT: char = '╯';

const KEY_WIDTH: usize = 5;

const CROSS_LEFT_AT: usize = 9;
const CROSS_MIDDLE_AT: usize = CROSS_LEFT_AT + KEY_WIDTH + 1;
const CROSS_RIGHT_AT: usize = CROSS_MIDDLE_AT + KEY_WIDTH + 1;
const GRID_AT: usize = CROSS_RIGHT_AT + KEY_WIDTH + 4;
const GRID_STEP: usize = KEY_WIDTH - 1;
const ENCODER_AT: usize = GRID_AT + GRID_WIDE * GRID_STEP + 5;
const GRID_WIDE: usize = 4;

const TOP_OF_UP: usize = 0;
const UP: usize = 1;
const MIDDLE: usize = 2;
const BOTTOM: usize = 3;
const UNDER: usize = 4;

const SCENES: [Control; GRID_WIDE] = [
    Control::Button(Button::FirstScene),
    Control::Button(Button::SecondScene),
    Control::Button(Button::ThirdScene),
    Control::Button(Button::FourthScene),
];

const ACTIONS: [Control; GRID_WIDE] = [
    Control::Button(Button::Play),
    Control::Button(Button::Stop),
    Control::Button(Button::Record),
    Control::Shift,
];

/// How a key is drawn: named by the panel, live but unnamed, or dead.
enum Look {
    Named(Hint),
    Live,
    Dead,
}

fn centred(
    frame: &mut Frame,
    row: usize,
    at: usize,
    width: usize,
    count: usize,
    glyphs: impl Iterator<Item = char>,
) {
    let offset = (width - count.min(width)) / 2;

    for (place, glyph) in glyphs.take(width).enumerate() {
        frame.set(at + offset + place, row, Cell::new(glyph));
    }
}

fn span(frame: &mut Frame, row: usize, at: usize, width: usize, left: char, right: char) {
    frame.set(at, row, Cell::new(left));
    for column in 1..width.saturating_sub(1) {
        frame.set(at + column, row, Cell::new(HORIZONTAL));
    }
    frame.set(at + width.saturating_sub(1), row, Cell::new(right));
}

fn face(frame: &mut Frame, row: usize, at: usize, look: Look) {
    let inside = KEY_WIDTH - 2;

    frame.set(at, row, Cell::new(VERTICAL));
    frame.set(at + KEY_WIDTH - 1, row, Cell::new(VERTICAL));

    match look {
        Look::Named(hint) => centred(
            frame,
            row,
            at + 1,
            inside,
            hint.glyphs().count(),
            hint.glyphs(),
        ),
        Look::Live => centred(
            frame,
            row,
            at + 1,
            inside,
            inside,
            std::iter::repeat_n(LIVE, inside),
        ),
        Look::Dead => centred(frame, row, at + 1, inside, 1, std::iter::once(DEAD)),
    }
}

/// Which controls a page answers, and so which keys are live on it.
///
/// A page answers a handful of controls and ignores the rest, and until it says
/// which, the only way to find out is to press one and watch. Declaring it is
/// what the screen is drawn from, so a control a page does not answer is drawn
/// dead rather than left out — the panel then reads the same everywhere, and a
/// key that does nothing here is a fact on the screen rather than a silence.
///
/// ```
/// use motif::device::{Button, Encoder};
/// use motif::ui::Legend;
///
/// let legend = Legend::blank().answering(Button::Play);
///
/// assert!(legend.answers(Button::Play));
/// assert!(!legend.answers(Encoder::Main));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Legend {
    answered: [bool; Control::ALL.len()],
}

impl Legend {
    /// How many rows a legend takes, along the bottom of the frame.
    ///
    /// It is permanent rather than an overlay a player summons: a legend that
    /// has to be asked for is one more thing to know about before it can help,
    /// and the pages this is drawn for are read while both hands are busy.
    ///
    /// Five rows is what the panel picture costs. A key is an edge with a glyph
    /// inside it, and the navigation cross is two of those stacked with a shared
    /// edge, which is five rows however tightly it is drawn. Everything else —
    /// the scene buttons with the transport under them, the encoder — sits
    /// beside the cross rather than below it, so it costs no rows of its own.
    ///
    /// A page draws into the whole frame and the legend is drawn over these
    /// rows afterwards, so what a page puts here does not survive the frame.
    pub const ROWS: usize = 5;

    /// A legend for a page that answers nothing yet.
    pub const fn blank() -> Self {
        Self {
            answered: [false; Control::ALL.len()],
        }
    }

    /// The same legend, with `control` doing something on this page.
    pub fn answering(mut self, control: impl Into<Control>) -> Self {
        self.answered[control.into().position()] = true;
        self
    }

    /// Whether `control` does anything on this page.
    pub fn answers(&self, control: impl Into<Control>) -> bool {
        self.answered[control.into().position()]
    }

    /// Draw the panel along the bottom [`ROWS`](Self::ROWS) rows of `frame`,
    /// with each live key wearing the glyph `panel` reaches it by.
    ///
    /// Every key keeps a place of its own, so nothing moves as the player
    /// crosses from page to page and only what is on the keys changes. A live
    /// key on a panel that offers no glyph — one whose keys are labelled under
    /// the player's hands — is filled instead, because the screen still has to
    /// say that it does something. The rows are cleared first, so nothing drawn
    /// underneath shows through the gaps between keys.
    pub fn draw(&self, frame: &mut Frame, panel: &impl Controls) {
        let screen = DeviceProfile::TARGET.screen;
        let top = screen.rows.saturating_sub(Self::ROWS);

        for row in top..screen.rows {
            for column in 0..screen.columns {
                frame.set(column, row, Cell::BLANK);
            }
        }

        self.draw_cross(frame, top, panel);
        self.draw_grid(frame, top, panel);
        self.draw_encoder(frame, top, panel);
    }

    fn draw_cross(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        span(
            frame,
            top + TOP_OF_UP,
            CROSS_MIDDLE_AT,
            KEY_WIDTH,
            TOP_LEFT,
            TOP_RIGHT,
        );
        face(
            frame,
            top + UP,
            CROSS_MIDDLE_AT,
            self.look_of(Button::Up, panel),
        );
        span(
            frame,
            top + MIDDLE,
            CROSS_MIDDLE_AT,
            KEY_WIDTH,
            JOIN_LEFT,
            JOIN_RIGHT,
        );

        for (at, button) in [
            (CROSS_LEFT_AT, Button::Left),
            (CROSS_MIDDLE_AT, Button::Down),
            (CROSS_RIGHT_AT, Button::Right),
        ] {
            if at != CROSS_MIDDLE_AT {
                span(frame, top + MIDDLE, at, KEY_WIDTH, TOP_LEFT, TOP_RIGHT);
            }
            face(frame, top + BOTTOM, at, self.look_of(button, panel));
            span(frame, top + UNDER, at, KEY_WIDTH, BOTTOM_LEFT, BOTTOM_RIGHT);
        }
    }

    fn draw_grid(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        grid_edge(frame, top + TOP_OF_UP, TOP_LEFT, JOIN_DOWN, TOP_RIGHT);
        grid_edge(frame, top + MIDDLE, JOIN_LEFT, CROSSING, JOIN_RIGHT);
        grid_edge(frame, top + UNDER, BOTTOM_LEFT, JOIN_UP, BOTTOM_RIGHT);

        for (place, control) in SCENES.into_iter().enumerate() {
            let at = GRID_AT + place * GRID_STEP;
            face(frame, top + UP, at, self.look_of(control, panel));
        }
        for (place, control) in ACTIONS.into_iter().enumerate() {
            let at = GRID_AT + place * GRID_STEP;
            face(frame, top + BOTTOM, at, self.look_of(control, panel));
        }
    }

    fn draw_encoder(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        span(
            frame,
            top + TOP_OF_UP,
            ENCODER_AT,
            KEY_WIDTH,
            ROUND_TOP_LEFT,
            ROUND_TOP_RIGHT,
        );
        face(
            frame,
            top + UP,
            ENCODER_AT,
            self.look_of(Encoder::Main, panel),
        );
        span(
            frame,
            top + MIDDLE,
            ENCODER_AT,
            KEY_WIDTH,
            ROUND_BOTTOM_LEFT,
            ROUND_BOTTOM_RIGHT,
        );
    }

    fn look_of(&self, control: impl Into<Control> + Copy, panel: &impl Controls) -> Look {
        if !self.answers(control) {
            return Look::Dead;
        }

        match panel.hint(control.into()) {
            Some(hint) => Look::Named(hint),
            None => Look::Live,
        }
    }
}

fn grid_edge(frame: &mut Frame, row: usize, left: char, join: char, right: char) {
    for place in 0..GRID_WIDE {
        let at = GRID_AT + place * GRID_STEP;
        let opens = if place == 0 { left } else { join };
        span(frame, row, at, KEY_WIDTH, opens, right);
    }
}

impl Default for Legend {
    fn default() -> Self {
        Self::blank()
    }
}
