//! The panel's edges, drawn in a terminal that is larger than the panel.

use std::io::Write;

use crate::device::DeviceProfile;
use crate::ui::{Frame, RenderError, Renderer};

use super::FrameWriter;

const TOP_LEFT: char = '┌';
const TOP_RIGHT: char = '┐';
const BOTTOM_LEFT: char = '└';
const BOTTOM_RIGHT: char = '┘';
const HORIZONTAL: char = '─';
const VERTICAL: char = '│';

/// A [`Renderer`] that draws the frame inside a border the size of the device's
/// screen.
///
/// A terminal is nearly always larger than the panel, so a frame drawn straight
/// into it has no edges: 40 columns of content in an 80-column window looks like
/// a layout with room to spare, and on the panel it is the whole screen. The
/// border is where the screen actually ends, which is what makes a layout
/// judgeable before there is hardware to judge it on.
///
/// The box is sized from [`DeviceProfile::TARGET`] and drawn at the top left.
/// The terminal is never asked how large it is — a frame is the size of the
/// panel wherever it is drawn, so a window with room to spare simply has space
/// around the box.
///
/// The border is drawn once, before the first frame, and again after a failed
/// write, on the same reasoning the frame itself is redrawn in full: a screen
/// that rejected part of a write is no longer known to show anything in
/// particular. Every frame after that writes only the cells that changed.
///
/// The border never passes through a [`Frame`]. The panel has no border, so
/// drawing one into the cells the application owns would put a decoration of
/// this backend's into every other backend's output.
pub struct Viewport<W: Write> {
    writer: FrameWriter<W>,
    bordered: bool,
}

impl<W: Write> Viewport<W> {
    /// A viewport whose border and first frame will both be written in full.
    pub fn new(sink: W) -> Self {
        Self {
            writer: FrameWriter::at(sink, 1, 1),
            bordered: false,
        }
    }

    /// What frames are being written to.
    pub fn sink(&self) -> &W {
        self.writer.sink()
    }

    fn draw_border(&mut self) -> Result<(), RenderError> {
        let screen = DeviceProfile::TARGET.screen;
        let span = String::from(HORIZONTAL).repeat(screen.columns);
        let right_edge = screen.columns + 2;

        write!(self.writer.sink, "\u{1b}[1;1H{TOP_LEFT}{span}{TOP_RIGHT}")
            .map_err(|_| RenderError::WriteFailed)?;

        for row in 0..screen.rows {
            let line = row + 2;
            write!(self.writer.sink, "\u{1b}[{line};1H{VERTICAL}")
                .map_err(|_| RenderError::WriteFailed)?;
            write!(self.writer.sink, "\u{1b}[{line};{right_edge}H{VERTICAL}")
                .map_err(|_| RenderError::WriteFailed)?;
        }

        write!(
            self.writer.sink,
            "\u{1b}[{};1H{BOTTOM_LEFT}{span}{BOTTOM_RIGHT}",
            screen.rows + 2
        )
        .map_err(|_| RenderError::WriteFailed)
    }
}

impl<W: Write> Renderer for Viewport<W> {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        if !self.bordered {
            self.draw_border()?;
            self.bordered = true;
        }

        if let Err(failed) = self.writer.render(frame) {
            self.bordered = false;
            return Err(failed);
        }

        Ok(())
    }
}
