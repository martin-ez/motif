//! The frame the UI draws into, and the traits a panel is reached through.
//!
//! The application fills a [`Frame`], hands it to a [`Renderer`], and takes
//! [`ControlEvent`]s back from [`Controls`]. Nothing crossing those traits may
//! name a terminal, an escape sequence, a key, or a crate implying one, and nor
//! may anything above them (invariant 4). The backends are re-exported here so
//! a program can construct one, the only thing it does that reveals its pick.
//!
//! A frame is the size of the device's screen, taken from
//! [`DeviceProfile::TARGET`]. What draws into it is handed a [`Region`] of it
//! rather than the whole thing, so chrome and a page cannot claim the same
//! cell. [`LevelMeter`] and [`Mode`] belong to no one screen.
//!
//! A [`Page`] is one screen and a [`Shell`] holds one per [`Mode`], so an `App`
//! is implemented once; what a [`Navigation`] makes an [`Intent`] of, it keeps.
//!
//! ```
//! use motif::ui::{Cell, Frame, NullRenderer, RenderError, Renderer};
//!
//! let mut frame = Frame::blank();
//! frame.set(0, 0, Cell::new('m'));
//!
//! let mut screen = NullRenderer::new();
//! screen.render(&frame)?;
//!
//! assert_eq!(screen.rendered().and_then(|f| f.get(0, 0)), Some(Cell::new('m')));
//! # Ok::<(), RenderError>(())
//! ```

use std::fmt;

use unicode_width::UnicodeWidthChar;

use crate::device::DeviceProfile;

mod chrome;
mod clock;
mod events;
mod hold;
mod input;
mod marks;
mod meter;
mod mode;
mod navigation;
#[cfg(feature = "frame-pace")]
mod pace;
mod page;
mod panel;
mod region;
mod shell;
mod terminal;

pub use chrome::Chrome;
pub use clock::{Clock, ScriptedClock, SystemClock};
pub use events::{App, EVENTS_PER_FRAME, EventLoop, Flow, RunReport};
pub use input::{ControlEvent, Controls, Hint, ScriptedControls, Turn};
pub use marks::Marks;
pub use meter::LevelMeter;
pub use mode::Mode;
pub use navigation::{Intent, Navigation, Scheme};
#[cfg(feature = "frame-pace")]
pub use pace::{Pace, PaceReader, PaceWriter, pace_meter};
pub use page::Page;
pub use panel::Panel;
pub use region::Region;
pub use shell::Shell;
pub use terminal::{CentredScreen, FrameWriter, KeyReader, TerminalScreen, Viewport};

/// One character cell of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    glyph: char,
    columns: u8,
}

impl Cell {
    /// An empty cell.
    pub const BLANK: Self = Self {
        glyph: ' ',
        columns: 1,
    };

    /// A cell showing `glyph`.
    ///
    /// A glyph that fills no column of its own is replaced by
    /// [`BLANK`](Self::BLANK). A control character moves the cursor rather than
    /// filling a cell, which would put a backend out of step with the frame and
    /// let an escape sequence through one cell at a time. A combining mark
    /// draws inside the cell before it rather than one of its own, which is
    /// [#193] and not this.
    ///
    /// [#193]: https://github.com/martin-ez/motif/issues/193
    pub fn new(glyph: char) -> Self {
        match UnicodeWidthChar::width(glyph) {
            Some(columns @ 1..=2) => Self {
                glyph,
                columns: columns as u8,
            },
            _ => Self::BLANK,
        }
    }

    /// The character in the cell.
    pub const fn glyph(self) -> char {
        self.glyph
    }

    /// How many columns the cell fills when it is drawn.
    ///
    /// Two for a glyph the East Asian Width property calls wide or fullwidth —
    /// most CJK and emoji — and one for the rest. The column a wide glyph takes
    /// beside itself answers zero: it belongs to the glyph before it and costs
    /// nothing of its own.
    ///
    /// This is a property of the glyph and not of any one screen. A wide glyph
    /// spans two cells of the device's panel as surely as two columns of a
    /// terminal, which is why the frame accounts for it and no backend does.
    pub const fn columns(self) -> usize {
        self.columns as usize
    }

    const fn continuing(self) -> Self {
        Self {
            glyph: self.glyph,
            columns: 0,
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::BLANK
    }
}

/// How many columns `text` fills when it is drawn.
///
/// What a layout measures a label against, so that a page can put something
/// beside it or align it against the far margin. Counting characters instead is
/// the mistake this exists to keep out of the pages.
///
/// ```
/// use motif::ui::columns_of;
///
/// assert_eq!(columns_of("motif"), 5);
/// assert_eq!(columns_of("オーディオ"), 10);
/// ```
pub fn columns_of(text: &str) -> usize {
    text.chars().map(|glyph| Cell::new(glyph).columns()).sum()
}

/// A screenful of cells for the application to fill.
///
/// Addressed by column and then row, both counted from zero at the top left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    cells: [Cell; DeviceProfile::TARGET.screen.cells()],
}

impl Frame {
    /// A frame of nothing but [`Cell::BLANK`].
    pub const fn blank() -> Self {
        Self {
            cells: [Cell::BLANK; DeviceProfile::TARGET.screen.cells()],
        }
    }

    /// The whole frame, as the region a run starts from.
    ///
    /// Whoever draws first is handed this and passes on what is left of it, so
    /// every row of the screen belongs to exactly one region.
    pub fn region(&mut self) -> Region<'_> {
        Region::new(&mut self.cells, DeviceProfile::TARGET.screen.columns)
    }

    /// Put `cell` at `column` and `row`.
    ///
    /// [`Region::set`]'s rules, applied to the whole screen.
    pub fn set(&mut self, column: usize, row: usize, cell: Cell) {
        self.region().set(column, row, cell);
    }

    /// Write `text` from `column` on `row`.
    ///
    /// [`Region::write`]'s rules, applied to the whole screen.
    pub fn write(&mut self, column: usize, row: usize, text: &str) {
        self.region().write(column, row, text);
    }

    /// The cell at `column` and `row`, or `None` if that is off the screen.
    ///
    /// The column beside a wide glyph answers that glyph, costing no columns of
    /// its own.
    pub fn get(&self, column: usize, row: usize) -> Option<Cell> {
        let screen = DeviceProfile::TARGET.screen;
        if column >= screen.columns || row >= screen.rows {
            return None;
        }

        self.cells.get(row * screen.columns + column).copied()
    }

    /// Every cell, row by row from the top.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::blank()
    }
}

/// Why a frame could not be put on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderError {
    /// The screen could not be written to.
    WriteFailed,
    /// There is no screen to draw on.
    Unavailable,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let described = match self {
            Self::WriteFailed => "the screen could not be written to",
            Self::Unavailable => "the screen is not available",
        };
        f.write_str(described)
    }
}

impl std::error::Error for RenderError {}

/// A screen a frame can be put on.
pub trait Renderer {
    /// Show `frame`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when the screen cannot be written to.
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError>;

    /// Show `panel` beside the screen, wherever this backend keeps it.
    ///
    /// Doing nothing is the default, and the right answer for the device: the
    /// picture stands in for keys a backend does not have, and a panel under
    /// the player's hands needs no drawing. It never reaches the [`Frame`],
    /// which is what keeps the rows a page draws into the same everywhere.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when the screen cannot be written to.
    fn show_panel(&mut self, _panel: &Panel) -> Result<(), RenderError> {
        Ok(())
    }
}

/// A renderer with no screen behind it, which keeps what it was given instead.
///
/// It exists so that drawing can be exercised where no screen is present, and
/// so that a test can read back what the application drew.
#[derive(Debug, Default)]
pub struct NullRenderer {
    rendered: Option<Frame>,
    shown: Option<Panel>,
}

impl NullRenderer {
    /// A renderer holding no frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// The most recent frame rendered, or `None` before the first.
    pub fn rendered(&self) -> Option<&Frame> {
        self.rendered.as_ref()
    }

    /// The most recent panel shown, or `None` before the first.
    pub fn shown(&self) -> Option<&Panel> {
        self.shown.as_ref()
    }
}

impl Renderer for NullRenderer {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        self.rendered = Some(frame.clone());
        Ok(())
    }

    fn show_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        self.shown = Some(panel.clone());
        Ok(())
    }
}
