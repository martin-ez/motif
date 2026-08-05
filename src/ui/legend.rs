//! The panel as a screen without one draws it: which controls do something here.
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
//!
//! The picture is a surface of its own rather than part of a [`Frame`]. The
//! device has the keys under the player's hands and the screen shows the page
//! alone, so a picture drawn into the frame would cost a page rows that the
//! hardware never charges it.

use crate::device::{Button, Control, Encoder};
use crate::ui::{Cell, Controls, Hint};

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

const CROSS_LEFT_AT: usize = 0;
const CROSS_MIDDLE_AT: usize = CROSS_LEFT_AT + KEY_WIDTH + 1;
const CROSS_RIGHT_AT: usize = CROSS_MIDDLE_AT + KEY_WIDTH + 1;
const GRID_AT: usize = CROSS_RIGHT_AT + KEY_WIDTH + 3;
const GRID_WIDE: usize = 4;
const ENCODER_AT: usize = GRID_AT + GRID_WIDE * KEY_WIDTH + 3;

const PANEL_COLUMNS: usize = ENCODER_AT + ENCODER_WIDTH;
const PANEL_ROWS: usize = KEY_ROWS * 2;

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

/// A picture of the panel, drawn beside a screen by a backend that has no
/// panel of its own.
///
/// A surface rather than a region of a [`Frame`](crate::ui::Frame): where it
/// goes is the backend's to decide, and a backend whose keys are real hardware
/// puts it nowhere at all.
///
/// Addressed by column and then row, both counted from zero at the top left,
/// as a frame is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    cells: [Cell; PANEL_COLUMNS * PANEL_ROWS],
}

impl Panel {
    /// How wide the picture is.
    ///
    /// The keys are what fix it: the navigation cross, the scene buttons with
    /// the transport under them, and the encoder beside, each key an edge with
    /// a glyph inside it. Nothing is padded out to a screen's width, because
    /// the picture does not know what it will be drawn next to.
    pub const COLUMNS: usize = PANEL_COLUMNS;

    /// How tall the picture is.
    ///
    /// A key is an edge with a glyph inside it, which is three rows, and the
    /// panel is two rows of keys deep wherever one looks at it. No key shares
    /// an edge with the key beside it, because a shared edge is one cell and
    /// the two keys it belongs to may not be drawn the same weight.
    pub const ROWS: usize = PANEL_ROWS;

    /// A picture of nothing at all.
    pub const fn blank() -> Self {
        Self {
            cells: [Cell::BLANK; PANEL_COLUMNS * PANEL_ROWS],
        }
    }

    /// The cell at `column` and `row`, or `None` if that is off the picture.
    pub fn get(&self, column: usize, row: usize) -> Option<Cell> {
        Self::position(column, row).map(|position| self.cells[position])
    }

    /// Every cell, row by row from the top.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    fn set(&mut self, column: usize, row: usize, cell: Cell) {
        if let Some(position) = Self::position(column, row) {
            self.cells[position] = cell;
        }
    }

    fn position(column: usize, row: usize) -> Option<usize> {
        if column >= Self::COLUMNS || row >= Self::ROWS {
            return None;
        }
        Some(row * Self::COLUMNS + column)
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::blank()
    }
}

fn centred(
    panel: &mut Panel,
    row: usize,
    at: usize,
    width: usize,
    count: usize,
    glyphs: impl Iterator<Item = char>,
) {
    let offset = (width - count.min(width)) / 2;

    for (place, glyph) in glyphs.take(width).enumerate() {
        panel.set(at + offset + place, row, Cell::new(glyph));
    }
}

fn span(panel: &mut Panel, row: usize, at: usize, width: usize, edges: &Edges, closing: bool) {
    let (left, right) = match closing {
        false => (edges.top_left, edges.top_right),
        true => (edges.bottom_left, edges.bottom_right),
    };

    panel.set(at, row, Cell::new(left));
    for column in 1..width.saturating_sub(1) {
        panel.set(at + column, row, Cell::new(edges.horizontal));
    }
    panel.set(at + width.saturating_sub(1), row, Cell::new(right));
}

fn face(panel: &mut Panel, row: usize, at: usize, width: usize, edges: &Edges, hint: Option<Hint>) {
    panel.set(at, row, Cell::new(edges.vertical));
    panel.set(at + width.saturating_sub(1), row, Cell::new(edges.vertical));

    if let Some(hint) = hint {
        centred(
            panel,
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

    /// The picture of the panel this page makes, each key wearing the glyph
    /// `controls` reaches it by and nothing else.
    ///
    /// A key the page answers is drawn with a heavy edge and one it ignores
    /// with a light one, so both are in the picture and it is a map of the
    /// panel wherever the player is. Every key keeps a place of its own, so
    /// nothing moves from page to page and only the weight changes. The encoder
    /// is rounded, never square, so that it does not read as a button — and it
    /// lights by doubling its edge rather than thickening it, there being no
    /// heavy rounded corner to draw.
    ///
    /// ```
    /// use motif::device::Button;
    /// use motif::ui::{Legend, Panel, ScriptedControls};
    ///
    /// let picture = Legend::blank().answering(Button::Play).picture(&ScriptedControls::new([]));
    ///
    /// assert_eq!(picture.cells().len(), Panel::COLUMNS * Panel::ROWS);
    /// ```
    pub fn picture(&self, controls: &impl Controls) -> Panel {
        let mut panel = Panel::blank();

        self.draw_cross(&mut panel, controls);
        self.draw_grid(&mut panel, controls);
        self.draw_key(
            &mut panel,
            0,
            ENCODER_AT,
            ENCODER_WIDTH,
            Control::Encoder(Encoder::Main),
            controls,
        );

        panel
    }

    fn draw_cross(&self, panel: &mut Panel, controls: &impl Controls) {
        self.draw_key(panel, 0, CROSS_MIDDLE_AT, KEY_WIDTH, Button::Up, controls);

        for (at, button) in [
            (CROSS_LEFT_AT, Button::Left),
            (CROSS_MIDDLE_AT, Button::Down),
            (CROSS_RIGHT_AT, Button::Right),
        ] {
            self.draw_key(panel, KEY_ROWS, at, KEY_WIDTH, button, controls);
        }
    }

    fn draw_grid(&self, panel: &mut Panel, controls: &impl Controls) {
        for (place, control) in SCENES.into_iter().enumerate() {
            let at = GRID_AT + place * KEY_WIDTH;
            self.draw_key(panel, 0, at, KEY_WIDTH, control, controls);
        }
        for (place, control) in ACTIONS.into_iter().enumerate() {
            let at = GRID_AT + place * KEY_WIDTH;
            self.draw_key(panel, KEY_ROWS, at, KEY_WIDTH, control, controls);
        }
    }

    fn draw_key(
        &self,
        panel: &mut Panel,
        top: usize,
        at: usize,
        width: usize,
        control: impl Into<Control> + Copy,
        controls: &impl Controls,
    ) {
        let control = control.into();
        let edges = edges_of(control, self.answers(control));

        span(panel, top, at, width, edges, false);
        face(panel, top + 1, at, width, edges, controls.hint(control));
        span(panel, top + 2, at, width, edges, true);
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
