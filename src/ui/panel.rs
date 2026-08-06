//! The panel as a screen without one draws it: which key reaches what.
//!
//! A backend says what to call the way each control is reached, in glyphs that
//! belong to the panel, which is what keeps a key out of every page.
//!
//! What it draws is a picture of the panel — the navigation cross, the scene
//! buttons with the transport under them, the encoder beside — and no words at
//! all. Every key wears the glyph that reaches it and goes heavy for the few
//! frames after its event arrives, the panel having no lamp under any key to
//! say so itself.
//!
//! The picture is a surface of its own, never part of a frame. The device's keys
//! cost the screen no rows, so the terminal's picture of them may not either.

use crate::device::{Button, Control, Encoder};
use crate::ui::{Cell, Controls, Hint, Marks, Turn};

/// Where on the picture a key is drawn, and how wide it is there.
#[derive(Debug, Clone, Copy)]
struct Seat {
    top: usize,
    at: usize,
    width: usize,
}

impl Seat {
    const fn key(top: usize, at: usize) -> Self {
        Self {
            top,
            at,
            width: KEY_WIDTH,
        }
    }

    const fn encoder(at: usize) -> Self {
        Self {
            top: 0,
            at,
            width: ENCODER_WIDTH,
        }
    }
}

/// The four corners and three sides a key is drawn with.
///
/// The two walls are separate because an encoder marks the side that moved, so
/// a key can be heavy down one edge and light down the other.
struct Edges {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    left: char,
    right: char,
}

const LIGHT: Edges = Edges {
    top_left: '┌',
    top_right: '┐',
    bottom_left: '└',
    bottom_right: '┘',
    horizontal: '─',
    left: '│',
    right: '│',
};

const HEAVY: Edges = Edges {
    top_left: '┏',
    top_right: '┓',
    bottom_left: '┗',
    bottom_right: '┛',
    horizontal: '━',
    left: '┃',
    right: '┃',
};

const ROUND: Edges = Edges {
    top_left: '╭',
    top_right: '╮',
    bottom_left: '╰',
    bottom_right: '╯',
    horizontal: '─',
    left: '│',
    right: '│',
};

const ROUND_LEFT_HEAVY: Edges = Edges {
    top_left: '┎',
    top_right: '╮',
    bottom_left: '┖',
    bottom_right: '╯',
    horizontal: '─',
    left: '┃',
    right: '│',
};

const ROUND_RIGHT_HEAVY: Edges = Edges {
    top_left: '╭',
    top_right: '┒',
    bottom_left: '╰',
    bottom_right: '┚',
    horizontal: '─',
    left: '│',
    right: '┃',
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

    /// The picture of the panel, each key wearing the glyph `controls` reaches
    /// it by and nothing else.
    ///
    /// Every key rests light and keeps a place of its own, so nothing moves
    /// from page to page. A key `marks` holds is drawn heavy: the panel this
    /// stands for has no lamp under any key, so the weight is spent on the one
    /// thing a player cannot otherwise tell, which is whether a press arrived.
    ///
    /// The encoder is rounded rather than square so it does not read as a
    /// button, and goes heavy down the side it was turned towards.
    ///
    /// ```
    /// use motif::ui::{Marks, Panel, ScriptedControls};
    ///
    /// let picture = Panel::showing(&ScriptedControls::new([]), Marks::none());
    ///
    /// assert_eq!(picture.cells().len(), Panel::COLUMNS * Panel::ROWS);
    /// ```
    pub fn showing(controls: &impl Controls, marks: Marks) -> Self {
        let mut panel = Self::blank();

        draw_cross(&mut panel, controls, marks);
        draw_grid(&mut panel, controls, marks);
        draw_key(
            &mut panel,
            Seat::encoder(ENCODER_AT),
            Control::Encoder(Encoder::Main),
            controls,
            marks,
        );

        panel
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
    panel.set(at, row, Cell::new(edges.left));
    panel.set(at + width.saturating_sub(1), row, Cell::new(edges.right));

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

fn draw_cross(panel: &mut Panel, controls: &impl Controls, marks: Marks) {
    draw_key(
        panel,
        Seat::key(0, CROSS_MIDDLE_AT),
        Button::Up,
        controls,
        marks,
    );

    for (at, button) in [
        (CROSS_LEFT_AT, Button::Left),
        (CROSS_MIDDLE_AT, Button::Down),
        (CROSS_RIGHT_AT, Button::Right),
    ] {
        draw_key(panel, Seat::key(KEY_ROWS, at), button, controls, marks);
    }
}

fn draw_grid(panel: &mut Panel, controls: &impl Controls, marks: Marks) {
    for (place, control) in SCENES.into_iter().enumerate() {
        let at = GRID_AT + place * KEY_WIDTH;
        draw_key(panel, Seat::key(0, at), control, controls, marks);
    }
    for (place, control) in ACTIONS.into_iter().enumerate() {
        let at = GRID_AT + place * KEY_WIDTH;
        draw_key(panel, Seat::key(KEY_ROWS, at), control, controls, marks);
    }
}

fn draw_key(
    panel: &mut Panel,
    seat: Seat,
    control: impl Into<Control> + Copy,
    controls: &impl Controls,
    marks: Marks,
) {
    let control = control.into();
    let edges = edges_of(control, marks);
    let Seat { top, at, width } = seat;

    span(panel, top, at, width, edges, false);
    face(panel, top + 1, at, width, edges, controls.hint(control));
    span(panel, top + 2, at, width, edges, true);
}

fn edges_of(control: Control, marks: Marks) -> &'static Edges {
    match control {
        Control::Button(_) if marks.marked(control) => &HEAVY,
        Control::Button(_) => &LIGHT,
        Control::Encoder(encoder) => match marks.turn(encoder) {
            Some(Turn::Clockwise) => &ROUND_RIGHT_HEAVY,
            Some(Turn::Anticlockwise) => &ROUND_LEFT_HEAVY,
            None => &ROUND,
        },
    }
}
