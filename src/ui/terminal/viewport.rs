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
const WIPE_SCREEN: &str = "\u{1b}[2J";

/// A [`Renderer`] that draws the frame inside a border the size of the device's
/// screen.
///
/// A terminal is nearly always larger than the panel, so a frame drawn straight
/// into it has no edges: 40 columns of content in an 80-column window looks like
/// a layout with room to spare, and on the panel it is the whole screen. The
/// border is where the screen actually ends, which is what makes a layout
/// judgeable before there is hardware to judge it on.
///
/// The box is sized from [`DeviceProfile::TARGET`]. Where it sits is the
/// caller's to say, through [`at`](Self::at) and [`place`](Self::place), and
/// only where — a frame is the size of the panel wherever it is drawn, so no
/// dimension here ever comes from the terminal. That split is what lets a
/// backend centre the box in the window it has without the screen the
/// application draws into changing size underneath it.
///
/// The border is drawn once, before the first frame, and again after a failed
/// write or a move, on the same reasoning the frame itself is redrawn in full:
/// a screen that rejected part of a write, or that the box has moved across, is
/// no longer known to show anything in particular. Every frame after that
/// writes only the cells that changed.
///
/// The border never passes through a [`Frame`]. The panel has no border, so
/// drawing one into the cells the application owns would put a decoration of
/// this backend's into every other backend's output.
pub struct Viewport<W: Write> {
    writer: FrameWriter<W>,
    bordered: bool,
    column: usize,
    row: usize,
    wipe: bool,
}

impl<W: Write> Viewport<W> {
    /// A viewport at the top left, whose border and first frame will both be
    /// written in full.
    pub fn new(sink: W) -> Self {
        Self::at(sink, 0, 0)
    }

    /// A viewport whose border's top-left corner sits at `column` and `row`,
    /// both counted from zero.
    ///
    /// The frame lands one cell inside that, so a viewport at the origin puts
    /// the panel's first cell at the terminal's second column and second row.
    pub fn at(sink: W, column: usize, row: usize) -> Self {
        Self {
            writer: FrameWriter::at(sink, column + 1, row + 1),
            bordered: false,
            column,
            row,
            wipe: false,
        }
    }

    /// Move the border's top-left corner to `column` and `row`.
    ///
    /// Moving wipes the screen and draws the border and the whole frame again
    /// on the next render, because the box has left a copy of itself where it
    /// used to be. Placing it where it already is does nothing at all, so a
    /// caller that checks the window every frame pays only for the frames the
    /// window actually changed on.
    pub fn place(&mut self, column: usize, row: usize) {
        if self.column == column && self.row == row {
            return;
        }

        self.column = column;
        self.row = row;
        self.writer.origin_column = column + 1;
        self.writer.origin_row = row + 1;
        self.writer.previous = None;
        self.bordered = false;
        self.wipe = true;
    }

    /// What frames are being written to.
    pub fn sink(&self) -> &W {
        self.writer.sink()
    }

    fn wipe_screen(&mut self) -> Result<(), RenderError> {
        write!(self.writer.sink, "{WIPE_SCREEN}").map_err(|_| RenderError::WriteFailed)?;
        self.wipe = false;
        Ok(())
    }

    fn draw_border(&mut self) -> Result<(), RenderError> {
        let screen = DeviceProfile::TARGET.screen;
        let span = String::from(HORIZONTAL).repeat(screen.columns);
        let left = self.column + 1;
        let right = self.column + screen.columns + 2;

        write!(
            self.writer.sink,
            "\u{1b}[{};{left}H{TOP_LEFT}{span}{TOP_RIGHT}",
            self.row + 1
        )
        .map_err(|_| RenderError::WriteFailed)?;

        for row in 0..screen.rows {
            let line = self.row + row + 2;
            write!(self.writer.sink, "\u{1b}[{line};{left}H{VERTICAL}")
                .map_err(|_| RenderError::WriteFailed)?;
            write!(self.writer.sink, "\u{1b}[{line};{right}H{VERTICAL}")
                .map_err(|_| RenderError::WriteFailed)?;
        }

        write!(
            self.writer.sink,
            "\u{1b}[{};{left}H{BOTTOM_LEFT}{span}{BOTTOM_RIGHT}",
            self.row + screen.rows + 2
        )
        .map_err(|_| RenderError::WriteFailed)
    }
}

impl<W: Write> Renderer for Viewport<W> {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        if self.wipe {
            self.wipe_screen()?;
        }

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
