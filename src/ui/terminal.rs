//! The terminal implementation of [`Renderer`].
//!
//! The only place in the project that knows what a terminal is. Everything
//! above draws into a [`Frame`] and never learns where that frame went, which
//! is what makes swapping this for a hardware screen a change to one directory.

use std::io::Write;

use crate::device::DeviceProfile;
use crate::ui::{Cell, Frame, RenderError, Renderer};

mod screen;

pub use screen::TerminalScreen;

/// A [`Renderer`] that writes a frame as escape sequences to anything taking
/// bytes.
///
/// Only cells that differ from the last frame are written, and cells that
/// changed next to each other on a row go out as one run after a single cursor
/// move. Writing the whole screen every frame is what makes a terminal UI feel
/// slow, and on the target device the screen is the slowest thing in the loop.
///
/// The first frame has nothing to compare against, so it is written in full. So
/// is the frame after a failed write, because a screen that rejected part of a
/// frame is no longer known to match anything.
pub struct FrameWriter<W: Write> {
    sink: W,
    previous: Option<Frame>,
}

impl<W: Write> FrameWriter<W> {
    /// A writer whose first frame will be written in full.
    pub fn new(sink: W) -> Self {
        Self {
            sink,
            previous: None,
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
        .filter(|column| differs(previous, frame, *column, row))
        .fold(Vec::new(), |mut runs, column| {
            let glyph = frame.get(column, row).unwrap_or(Cell::BLANK).glyph();
            match runs.last_mut() {
                Some(run) if run.ends_at == column => {
                    run.glyphs.push(glyph);
                    run.ends_at = column + 1;
                }
                _ => runs.push(Run {
                    starts_at: column,
                    ends_at: column + 1,
                    glyphs: String::from(glyph),
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
                    row + 1,
                    run.starts_at + 1,
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
