//! The screen's edges and the keys under them, drawn in a terminal that is
//! larger than the device.

use std::io::Write;

use crate::device::DeviceProfile;
use crate::ui::{Cell, Frame, Panel, RenderError, Renderer};

use super::FrameWriter;

const TOP_LEFT: char = '┌';
const TOP_RIGHT: char = '┐';
const BOTTOM_LEFT: char = '└';
const BOTTOM_RIGHT: char = '┘';
const HORIZONTAL: char = '─';
const VERTICAL: char = '│';
const WIPE_SCREEN: &str = "\u{1b}[2J";
const PANEL_GAP: usize = 1;

/// A [`Renderer`] that draws the frame inside a border the size of the device's
/// screen, and the keys the terminal stands in for under it.
///
/// A terminal is nearly always larger than the panel, so a frame drawn straight
/// into it has no edges and a layout is judged against the wrong screen.
///
/// The box is sized from [`DeviceProfile::TARGET`]; only where it sits is the
/// caller's, through [`at`](Self::at) and [`place`](Self::place). Box and keys
/// are drawn again after a failed write or a move, and neither passes through a
/// [`Frame`] — the device has no border, and its keys cost the screen no rows.
pub struct Viewport<W: Write> {
    writer: FrameWriter<W>,
    bordered: bool,
    column: usize,
    row: usize,
    wipe: bool,
    panel: Option<Panel>,
}

impl<W: Write> Viewport<W> {
    /// How many columns of the terminal a viewport covers: the screen and the
    /// border around it.
    pub const COLUMNS: usize = DeviceProfile::TARGET.screen.columns + 2;

    /// How many rows of the terminal a viewport covers: the screen, the border
    /// around it, and the panel under that.
    pub const ROWS: usize = DeviceProfile::TARGET.screen.rows + 2 + PANEL_GAP + Panel::ROWS;

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
            panel: None,
        }
    }

    /// Move the border's top-left corner to `column` and `row`.
    ///
    /// Moving wipes the screen and draws the border, the whole frame and the
    /// panel again on the next render, because the box has left a copy of
    /// itself where it used to be. Placing it where it already is does nothing
    /// at all, so a caller that checks the window every frame pays only for the
    /// frames the window actually changed on.
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
        self.panel = None;
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

    fn draw_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        let screen = DeviceProfile::TARGET.screen;
        let left = self.column + (Self::COLUMNS - Panel::COLUMNS) / 2 + 1;
        let top = self.row + screen.rows + 2 + PANEL_GAP;

        for row in 0..Panel::ROWS {
            let keys: String = (0..Panel::COLUMNS)
                .map(|column| panel.get(column, row).unwrap_or(Cell::BLANK).glyph())
                .collect();

            write!(self.writer.sink, "\u{1b}[{};{left}H{keys}", top + row + 1)
                .map_err(|_| RenderError::WriteFailed)?;
        }

        self.writer
            .sink
            .flush()
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
            self.panel = None;
            return Err(failed);
        }

        Ok(())
    }

    fn show_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        if self.panel.as_ref() == Some(panel) {
            return Ok(());
        }

        self.panel = None;
        self.draw_panel(panel)?;
        self.panel = Some(panel.clone());

        Ok(())
    }
}
