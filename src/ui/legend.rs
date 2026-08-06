//! The panel as a screen without one draws it: which controls do something here.
//!
//! Two halves meet here and neither knows the other. A page declares which
//! controls it answers; a backend says what to call the way each one is reached,
//! in glyphs that belong to the panel. The legend puts the two together, which
//! is what keeps a key out of every page and a page out of every backend.
//!
//! What it draws is a picture of the panel — the navigation cross, the scene
//! buttons with the transport under them, the encoder beside — and no words at
//! all. Every key wears the glyph that reaches it, drawn with a heavy edge where
//! the page answers it and a light one where it does not: a screen that has to
//! explain its controls in prose can go on being unreadable.
//!
//! The picture is a surface of its own, never part of a frame. The device's keys
//! cost the screen no rows, so the terminal's picture of them may not either.

use crate::device::{Button, Control, Encoder};
use crate::ui::{Cell, Controls, Hint, Marks};

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

const SOLID: Edges = Edges {
    top_left: '█',
    top_right: '█',
    bottom_left: '█',
    bottom_right: '█',
    horizontal: '█',
    vertical: '█',
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
/// what the [`picture`](Self::picture) is drawn from, so a control a page does
/// not answer is drawn light rather than left out — the panel then reads the
/// same everywhere, and a key that does nothing here is a fact the player can
/// see rather than a silence.
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

    /// The same legend, also answering everything `other` answers.
    ///
    /// What puts two declarations together. A page declares what it answers and
    /// nothing else, so a control the page never sees — the way out of the run,
    /// a gesture that navigates — is declared by whoever keeps it and joined on
    /// here.
    ///
    /// ```
    /// use motif::device::{Button, Encoder};
    /// use motif::ui::Legend;
    ///
    /// let legend = Legend::blank()
    ///     .answering(Button::Play)
    ///     .also_answering(Legend::blank().answering(Encoder::Main));
    ///
    /// assert!(legend.answers(Button::Play));
    /// assert!(legend.answers(Encoder::Main));
    /// ```
    pub fn also_answering(mut self, other: Self) -> Self {
        for (answered, also) in self.answered.iter_mut().zip(other.answered) {
            *answered |= also;
        }

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
    /// with a light one, and every key keeps a place of its own. The encoder is
    /// rounded rather than square so it does not read as a button, and lights
    /// by doubling its edge: there is no heavy rounded corner.
    ///
    /// A key `marks` holds is drawn solid, whatever weight it rests at — there
    /// is no fourth family of corners, and the weight reads either side of it.
    ///
    /// ```
    /// use motif::device::Button;
    /// use motif::ui::{Legend, Marks, Panel, ScriptedControls};
    ///
    /// let legend = Legend::blank().answering(Button::Play);
    /// let picture = legend.picture(&ScriptedControls::new([]), Marks::none());
    ///
    /// assert_eq!(picture.cells().len(), Panel::COLUMNS * Panel::ROWS);
    /// ```
    pub fn picture(&self, controls: &impl Controls, marks: Marks) -> Panel {
        let mut panel = Panel::blank();

        self.draw_cross(&mut panel, controls, marks);
        self.draw_grid(&mut panel, controls, marks);
        self.draw_key(
            &mut panel,
            Seat::encoder(ENCODER_AT),
            Control::Encoder(Encoder::Main),
            controls,
            marks,
        );

        panel
    }

    fn draw_cross(&self, panel: &mut Panel, controls: &impl Controls, marks: Marks) {
        self.draw_key(
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
            self.draw_key(panel, Seat::key(KEY_ROWS, at), button, controls, marks);
        }
    }

    fn draw_grid(&self, panel: &mut Panel, controls: &impl Controls, marks: Marks) {
        for (place, control) in SCENES.into_iter().enumerate() {
            let at = GRID_AT + place * KEY_WIDTH;
            self.draw_key(panel, Seat::key(0, at), control, controls, marks);
        }
        for (place, control) in ACTIONS.into_iter().enumerate() {
            let at = GRID_AT + place * KEY_WIDTH;
            self.draw_key(panel, Seat::key(KEY_ROWS, at), control, controls, marks);
        }
    }

    fn draw_key(
        &self,
        panel: &mut Panel,
        seat: Seat,
        control: impl Into<Control> + Copy,
        controls: &impl Controls,
        marks: Marks,
    ) {
        let control = control.into();
        let edges = edges_of(control, self.answers(control), marks.marked(control));
        let Seat { top, at, width } = seat;

        span(panel, top, at, width, edges, false);
        face(panel, top + 1, at, width, edges, controls.hint(control));
        span(panel, top + 2, at, width, edges, true);
    }
}

fn edges_of(control: Control, lit: bool, marked: bool) -> &'static Edges {
    match (control, lit, marked) {
        (_, _, true) => &SOLID,
        (Control::Encoder(_), false, false) => &ROUND,
        (Control::Encoder(_), true, false) => &DOUBLED,
        (Control::Button(_), false, false) => &LIGHT,
        (Control::Button(_), true, false) => &HEAVY,
    }
}

impl Default for Legend {
    fn default() -> Self {
        Self::blank()
    }
}
