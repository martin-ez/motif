//! The frame the UI draws into, and the trait a screen is reached through.
//!
//! The application fills a [`Frame`] and hands it to a [`Renderer`]. Which
//! screen is on the other side is not visible from here, and must not become
//! visible: no type in this module, or above it, may name a terminal, an escape
//! sequence, or a crate that implies either. A terminal is one backend, and the
//! screen being aimed at is not a terminal.
//!
//! A frame is the size of the device's screen, taken from
//! [`DeviceProfile::TARGET`], so it is a fixed-size array rather than an
//! allocation that grows with whatever the host reports.
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

use crate::device::DeviceProfile;

/// One character cell of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    glyph: char,
}

impl Cell {
    /// An empty cell.
    pub const BLANK: Self = Self::new(' ');

    /// A cell showing `glyph`.
    pub const fn new(glyph: char) -> Self {
        Self { glyph }
    }

    /// The character in the cell.
    pub const fn glyph(self) -> char {
        self.glyph
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::BLANK
    }
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

    /// Put `cell` at `column` and `row`.
    ///
    /// A position off the screen is dropped. Drawing clips at the edge rather
    /// than failing, because a widget that runs past the margin is a layout to
    /// fix and not a reason to stop rendering the rest of the frame.
    pub fn set(&mut self, column: usize, row: usize, cell: Cell) {
        if let Some(position) = Self::position(column, row) {
            self.cells[position] = cell;
        }
    }

    /// The cell at `column` and `row`, or `None` if that is off the screen.
    pub fn get(&self, column: usize, row: usize) -> Option<Cell> {
        Self::position(column, row).map(|position| self.cells[position])
    }

    /// Every cell, row by row from the top.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    fn position(column: usize, row: usize) -> Option<usize> {
        let screen = DeviceProfile::TARGET.screen;
        if column >= screen.columns || row >= screen.rows {
            return None;
        }
        Some(row * screen.columns + column)
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
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let described = match self {
            Self::WriteFailed => "the screen could not be written to",
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
}

/// A renderer with no screen behind it, which keeps the frame instead.
///
/// It exists so that drawing can be exercised where no screen is present, and
/// so that a test can read back what the application drew.
#[derive(Debug, Default)]
pub struct NullRenderer {
    rendered: Option<Frame>,
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
}

impl Renderer for NullRenderer {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        self.rendered = Some(frame.clone());
        Ok(())
    }
}
