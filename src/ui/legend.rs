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
//! beside — and no words at all. Every key wears the glyph that reaches it and
//! nothing else, so the picture is a map of the panel wherever the player is;
//! the keys the page answers are drawn with a heavy edge and the rest with a
//! light one. That is deliberately all it says: a screen that explains its
//! controls in prose can go on being unreadable, where one that says only
//! *which* controls are live has to make the rest obvious where the player is
//! already looking.

use crate::device::{Button, Control, DeviceProfile, Encoder};
use crate::ui::{Cell, Controls, Frame, Hint};

/// The four corners and two sides a key is drawn with.
struct Edges {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
}

const LIGHT: Edges = Edges {
    top_left: '┌',
    top_right: '┐',
    bottom_left: '└',
    bottom_right: '┘',
    horizontal: '─',
    vertical: '│',
};

const HEAVY: Edges = Edges {
    top_left: '┏',
    top_right: '┓',
    bottom_left: '┗',
    bottom_right: '┛',
    horizontal: '━',
    vertical: '┃',
};

const ROUND: Edges = Edges {
    top_left: '╭',
    top_right: '╮',
    bottom_left: '╰',
    bottom_right: '╯',
    horizontal: '─',
    vertical: '│',
};

const DOUBLED: Edges = Edges {
    top_left: '╔',
    top_right: '╗',
    bottom_left: '╚',
    bottom_right: '╝',
    horizontal: '═',
    vertical: '║',
};

const KEY_WIDTH: usize = 5;
const ENCODER_WIDTH: usize = 7;
const KEY_ROWS: usize = 3;

const CROSS_LEFT_AT: usize = 9;
const CROSS_MIDDLE_AT: usize = CROSS_LEFT_AT + KEY_WIDTH + 1;
const CROSS_RIGHT_AT: usize = CROSS_MIDDLE_AT + KEY_WIDTH + 1;
const GRID_AT: usize = CROSS_RIGHT_AT + KEY_WIDTH + 3;
const GRID_WIDE: usize = 4;
const ENCODER_AT: usize = GRID_AT + GRID_WIDE * KEY_WIDTH + 3;

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
    Control::Button(Button::Shift),
];

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

fn span(frame: &mut Frame, row: usize, at: usize, width: usize, edges: &Edges, closing: bool) {
    let (left, right) = match closing {
        false => (edges.top_left, edges.top_right),
        true => (edges.bottom_left, edges.bottom_right),
    };

    frame.set(at, row, Cell::new(left));
    for column in 1..width.saturating_sub(1) {
        frame.set(at + column, row, Cell::new(edges.horizontal));
    }
    frame.set(at + width.saturating_sub(1), row, Cell::new(right));
}

fn face(frame: &mut Frame, row: usize, at: usize, width: usize, edges: &Edges, hint: Option<Hint>) {
    frame.set(at, row, Cell::new(edges.vertical));
    frame.set(at + width.saturating_sub(1), row, Cell::new(edges.vertical));

    if let Some(hint) = hint {
        centred(
            frame,
            row,
            at + 1,
            width.saturating_sub(2),
            hint.glyphs().count(),
            hint.glyphs(),
        );
    }
}

/// Which controls a page answers, and so which keys are live on it.
///
/// A page answers a handful of controls and ignores the rest, and until it says
/// which, the only way to find out is to press one and watch. Declaring it is
/// what the screen is drawn from, so a control a page does not answer is drawn
/// light rather than left out — the panel then reads the same everywhere, and a
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
    /// Six rows is what the panel picture costs. A key is an edge with a glyph
    /// inside it, which is three rows, and the panel is two rows of keys deep
    /// wherever one looks at it: the navigation cross, and the scene buttons
    /// with the transport under them. No key shares an edge with the key beside
    /// it, because a shared edge is one cell and the two keys it belongs to may
    /// not be drawn the same weight.
    ///
    /// A page draws into the whole frame and the legend is drawn over these
    /// rows afterwards, so what a page puts here does not survive the frame.
    pub const ROWS: usize = KEY_ROWS * 2;

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
    /// each key wearing the glyph `panel` reaches it by and nothing else.
    ///
    /// A key the page answers is drawn with a heavy edge and one it ignores
    /// with a light one, so both are on the screen and the picture is a map of
    /// the panel wherever the player is. Every key keeps a place of its own, so
    /// nothing moves from page to page and only the weight changes. The encoder
    /// is rounded, never square, so that it does not read as a button — and it
    /// lights by doubling its edge rather than thickening it, there being no
    /// heavy rounded corner to draw.
    ///
    /// The rows are cleared first, so nothing drawn underneath shows through
    /// the gaps between keys.
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
        self.draw_key(
            frame,
            top,
            ENCODER_AT,
            ENCODER_WIDTH,
            Control::Encoder(Encoder::Main),
            panel,
        );
    }

    fn draw_cross(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        self.draw_key(frame, top, CROSS_MIDDLE_AT, KEY_WIDTH, Button::Up, panel);

        for (at, button) in [
            (CROSS_LEFT_AT, Button::Left),
            (CROSS_MIDDLE_AT, Button::Down),
            (CROSS_RIGHT_AT, Button::Right),
        ] {
            self.draw_key(frame, top + KEY_ROWS, at, KEY_WIDTH, button, panel);
        }
    }

    fn draw_grid(&self, frame: &mut Frame, top: usize, panel: &impl Controls) {
        for (place, control) in SCENES.into_iter().enumerate() {
            let at = GRID_AT + place * KEY_WIDTH;
            self.draw_key(frame, top, at, KEY_WIDTH, control, panel);
        }
        for (place, control) in ACTIONS.into_iter().enumerate() {
            let at = GRID_AT + place * KEY_WIDTH;
            self.draw_key(frame, top + KEY_ROWS, at, KEY_WIDTH, control, panel);
        }
    }

    fn draw_key(
        &self,
        frame: &mut Frame,
        top: usize,
        at: usize,
        width: usize,
        control: impl Into<Control> + Copy,
        panel: &impl Controls,
    ) {
        let control = control.into();
        let edges = edges_of(control, self.answers(control));

        span(frame, top, at, width, edges, false);
        face(frame, top + 1, at, width, edges, panel.hint(control));
        span(frame, top + 2, at, width, edges, true);
    }
}

fn edges_of(control: Control, lit: bool) -> &'static Edges {
    match (control, lit) {
        (Control::Encoder(_), false) => &ROUND,
        (Control::Encoder(_), true) => &DOUBLED,
        (Control::Button(_), false) => &LIGHT,
        (Control::Button(_), true) => &HEAVY,
    }
}

impl Default for Legend {
    fn default() -> Self {
        Self::blank()
    }
}
