//! The terminal implementation of [`Renderer`] and [`Controls`](super::Controls).
//!
//! The only place in the project that knows what a terminal is. Everything
//! above draws into a [`Frame`], never learns where that frame went, and is
//! handed controls rather than keys — which is what makes swapping this for a
//! hardware panel a change to one directory.

use std::io::Write;

use crate::device::DeviceProfile;
use crate::ui::{Cell, Frame, RenderError, Renderer};

mod keys;
mod screen;
mod viewport;

pub use keys::KeyReader;
pub use screen::{CentredScreen, TerminalScreen};
pub use viewport::Viewport;

/// A [`Renderer`] that writes a frame as escape sequences to anything taking
/// bytes.
///
/// Only cells that differ from the last frame are written, and cells that
/// changed next to each other on a row go out as one run after a single cursor
/// move. On the target device the screen is the slowest thing in the loop.
///
/// The first frame has nothing to compare against, so it is written in full, as
/// is the frame after a failed write.
pub struct FrameWriter<W: Write> {
    sink: W,
    previous: Option<Frame>,
    origin_column: usize,
    origin_row: usize,
}

impl<W: Write> FrameWriter<W> {
    /// A writer whose first frame will be written in full, at the top left of
    /// the screen.
    pub fn new(sink: W) -> Self {
        Self::at(sink, 0, 0)
    }

    /// A writer that puts the frame's top-left cell at `origin_column` and
    /// `origin_row`, both counted from zero.
    ///
    /// The offset is the caller's to state rather than the terminal's to
    /// report: a frame is the size of the panel wherever it is drawn, so
    /// nothing here asks how large the terminal is. It exists so that a
    /// [`Viewport`] can leave room for the border it draws around the frame.
    pub fn at(sink: W, origin_column: usize, origin_row: usize) -> Self {
        Self {
            sink,
            previous: None,
            origin_column,
            origin_row,
        }
    }

    /// What frames are being written to.
    pub fn sink(&self) -> &W {
        &self.sink
    }
}

struct Run {
    starts_at: usize,
    ends_at: usize,
    glyphs: String,
}

fn differs(previous: Option<&Frame>, frame: &Frame, column: usize, row: usize) -> bool {
    match previous {
        None => true,
        Some(previous) => previous.get(column, row) != frame.get(column, row),
    }
}

fn changed_runs(previous: Option<&Frame>, frame: &Frame, row: usize) -> Vec<Run> {
    let screen = DeviceProfile::TARGET.screen;

    (0..screen.columns)
        .map(|column| (column, frame.get(column, row).unwrap_or(Cell::BLANK)))
        .filter(|(_, cell)| cell.columns() > 0)
        .filter(|(column, _)| differs(previous, frame, *column, row))
        .fold(Vec::new(), |mut runs, (column, cell)| {
            let ends_at = column + cell.columns();
            match runs.last_mut() {
                Some(run) if run.ends_at == column => {
                    run.glyphs.push(cell.glyph());
                    run.ends_at = ends_at;
                }
                _ => runs.push(Run {
                    starts_at: column,
                    ends_at,
                    glyphs: String::from(cell.glyph()),
                }),
            }
            runs
        })
}

impl<W: Write> Renderer for FrameWriter<W> {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        let screen = DeviceProfile::TARGET.screen;
        let previous = self.previous.take();

        for row in 0..screen.rows {
            for run in changed_runs(previous.as_ref(), frame, row) {
                write!(
                    self.sink,
                    "\u{1b}[{};{}H{}",
                    self.origin_row + row + 1,
                    self.origin_column + run.starts_at + 1,
                    run.glyphs
                )
                .map_err(|_| RenderError::WriteFailed)?;
            }
        }

        self.sink.flush().map_err(|_| RenderError::WriteFailed)?;
        self.previous = Some(frame.clone());
        Ok(())
    }
}
