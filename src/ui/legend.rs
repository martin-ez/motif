//! What a page's controls do, drawn where the player can read it.
//!
//! Two halves meet here and neither knows the other. A page declares what its
//! controls mean, in words that belong to the page; a backend says what to call
//! the way each one is reached, in glyphs that belong to the panel. The legend
//! puts the two together, which is what keeps a key out of every page and a
//! meaning out of every backend.

use crate::device::{Button, Control, DeviceProfile, Encoder};
use crate::ui::{Cell, Controls, Frame, Hint};

const UNAVAILABLE: &str = "-";
const GAP: usize = 1;

fn place(
    frame: &mut Frame,
    mut column: usize,
    ends_at: usize,
    row: usize,
    glyphs: impl Iterator<Item = char>,
) -> usize {
    for glyph in glyphs {
        if column >= ends_at {
            break;
        }
        frame.set(column, row, Cell::new(glyph));
        column += 1;
    }

    column
}

fn entry(
    frame: &mut Frame,
    row: usize,
    starts_at: usize,
    ends_at: usize,
    hint: Option<Hint>,
    meaning: &str,
) {
    let mut column = starts_at;

    if let Some(hint) = hint {
        column = place(frame, column, ends_at, row, hint.glyphs());
        column = place(frame, column, ends_at, row, std::iter::once(' '));
    }

    place(frame, column, ends_at, row, meaning.chars());
}

/// What each control on the panel does on one page.
///
/// A page answers a handful of controls and ignores the rest, and until it says
/// which, the only way to find out is to press one and watch. Declaring it is
/// what the screen is drawn from, so a control a page does not answer is drawn
/// as unavailable rather than left out — the panel then reads the same
/// everywhere, and a control doing nothing here is a fact on the screen rather
/// than a silence.
///
/// ```
/// use motif::device::{Button, Encoder};
/// use motif::ui::Legend;
///
/// let legend = Legend::blank().naming(Button::Play, "play");
///
/// assert_eq!(legend.meaning(Button::Play), Some("play"));
/// assert_eq!(legend.meaning(Encoder::First), None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Legend {
    meanings: [Option<&'static str>; Control::ALL.len()],
}

impl Legend {
    /// How many rows a legend takes, along the bottom of the frame.
    ///
    /// It is permanent rather than an overlay a player summons: a legend that
    /// has to be asked for is one more thing to know about before it can help,
    /// and the pages this is drawn for are read while both hands are busy. Two
    /// rows because the panel has eleven controls, which do not fit across one
    /// screen wide enough to say anything useful about them — buttons take the
    /// last row and encoders the one above it.
    ///
    /// A page draws into the whole frame and the legend is drawn over these
    /// rows afterwards, so what a page puts here does not survive the frame.
    pub const ROWS: usize = 2;

    /// A legend for a page that answers nothing yet.
    pub const fn blank() -> Self {
        Self {
            meanings: [None; Control::ALL.len()],
        }
    }

    /// The same legend, with `control` meaning `meaning` on this page.
    ///
    /// Declaring a control twice keeps the later meaning, so a page built on
    /// another page's legend can say what it does differently.
    pub fn naming(mut self, control: impl Into<Control>, meaning: &'static str) -> Self {
        self.meanings[control.into().position()] = Some(meaning);
        self
    }

    /// What `control` does on this page, or `None` where the page ignores it.
    pub fn meaning(&self, control: impl Into<Control>) -> Option<&'static str> {
        self.meanings[control.into().position()]
    }

    /// Draw the legend along the bottom [`ROWS`](Self::ROWS) rows of `frame`,
    /// naming each control the way `panel` reaches it.
    ///
    /// Every control keeps a column of its own whatever it means, so an entry
    /// does not move when a page changes what it says; a meaning too long for
    /// its entry is clipped rather than pushing the next one along. The rows are
    /// filled edge to edge, so nothing drawn underneath shows through the gaps.
    pub fn draw(&self, frame: &mut Frame, panel: &impl Controls) {
        let rows = DeviceProfile::TARGET.screen.rows;

        self.draw_row(
            frame,
            rows.saturating_sub(Self::ROWS),
            &Encoder::ALL.map(Control::Encoder),
            panel,
        );
        self.draw_row(
            frame,
            rows.saturating_sub(1),
            &Button::ALL.map(Control::Button),
            panel,
        );
    }

    fn draw_row(&self, frame: &mut Frame, row: usize, controls: &[Control], panel: &impl Controls) {
        let columns = DeviceProfile::TARGET.screen.columns;
        let width = columns / controls.len().max(1);

        for column in 0..columns {
            frame.set(column, row, Cell::BLANK);
        }

        for (at, control) in controls.iter().enumerate() {
            let starts_at = at * width;
            entry(
                frame,
                row,
                starts_at,
                starts_at + width.saturating_sub(GAP),
                panel.hint(*control),
                self.meaning(*control).unwrap_or(UNAVAILABLE),
            );
        }
    }
}

impl Default for Legend {
    fn default() -> Self {
        Self::blank()
    }
}
