//! The part of a frame something is allowed to draw into.
//!
//! Chrome takes the rows it needs off the region it was handed and passes the
//! rest inward, so the cells behind two regions are disjoint slices and nothing
//! drawn into one can reach the other. That is what makes the rule the compiler
//! holds rather than a convention: a page is given a region and has no way to
//! address a row outside it.
//!
//! A region is whole rows rather than any rectangle, because the columns are
//! what a wide glyph spans. A band never cuts one in half, so the rules for a
//! glyph beside the margin are the frame's rules unchanged.

use crate::ui::Cell;

/// A band of whole rows of a [`Frame`](crate::ui::Frame).
///
/// Addressed from its own top left, and clipping at its own edges: a page draws
/// as though its region were the screen, and what falls outside is dropped
/// rather than landing on someone else's row.
///
/// ```
/// use motif::ui::{Cell, Frame};
///
/// let mut frame = Frame::blank();
/// let (mut heading, mut body) = frame.region().split_top(1);
///
/// heading.write(0, 0, "motif");
/// body.write(0, 0, "the rest");
///
/// assert_eq!(frame.get(0, 1), Some(Cell::new('t')));
/// ```
#[derive(Debug)]
pub struct Region<'a> {
    cells: &'a mut [Cell],
    columns: usize,
}

impl<'a> Region<'a> {
    /// How many columns across the region is.
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// How many rows down the region is.
    pub const fn rows(&self) -> usize {
        self.cells.len() / self.columns
    }

    /// Put `cell` at `column` and `row` of this region.
    ///
    /// A position outside the region is dropped, and so is a wide cell with no
    /// column beside it to take its other half. Drawing clips at the region's
    /// edge rather than failing: a widget past the margin is a layout to fix,
    /// not a reason to stop rendering the frame.
    ///
    /// A wide cell claims the column beside it, and either half is cleared when
    /// the other is written over. A backend drawing half a glyph would move its
    /// cursor out of step with the frame and shift the rest of the row.
    pub fn set(&mut self, column: usize, row: usize, cell: Cell) {
        let Some(position) = self.position(column, row) else {
            return;
        };
        let wide = cell.columns() == 2;
        if wide && self.position(column + 1, row).is_none() {
            return;
        }

        self.separate(column, row);
        if wide {
            self.separate(column + 1, row);
        }

        self.cells[position] = cell;
        if wide {
            self.cells[position + 1] = cell.continuing();
        }
    }

    /// Write `text` from `column` on `row` of this region.
    ///
    /// Each glyph starts where the one before it ended, so a wide glyph moves
    /// what follows two columns on rather than one. The row stops at the
    /// region's margin — the glyph that would cross it is dropped, along with
    /// the rest.
    ///
    /// The one place text becomes cells, so that the width of a glyph is
    /// accounted for wherever a page draws a label rather than in each page
    /// that remembers to.
    pub fn write(&mut self, column: usize, row: usize, text: &str) {
        let mut at = column;

        for glyph in text.chars() {
            let cell = Cell::new(glyph);
            self.set(at, row, cell);
            at += cell.columns();
        }
    }

    /// The cell at `column` and `row` of this region, or `None` outside it.
    ///
    /// The column beside a wide glyph answers that glyph, costing no columns of
    /// its own.
    pub fn get(&self, column: usize, row: usize) -> Option<Cell> {
        self.position(column, row)
            .map(|position| self.cells[position])
    }

    /// The first `rows` of this region, and everything under them.
    ///
    /// More rows than the region has takes all of them and leaves a remainder of
    /// none, so a layout too tall for its screen draws what fits instead of
    /// panicking.
    pub fn split_top(self, rows: usize) -> (Self, Self) {
        let taken = self.spanning(rows);
        let (top, rest) = self.cells.split_at_mut(taken);

        (
            Self {
                cells: top,
                columns: self.columns,
            },
            Self {
                cells: rest,
                columns: self.columns,
            },
        )
    }

    /// Everything above the last `rows` of this region, and those rows.
    ///
    /// The remainder comes first, so that the pair reads top to bottom the way
    /// [`split_top`](Self::split_top)'s does.
    pub fn split_bottom(self, rows: usize) -> (Self, Self) {
        let taken = self.spanning(rows);
        let at = self.cells.len() - taken;
        let (rest, bottom) = self.cells.split_at_mut(at);

        (
            Self {
                cells: rest,
                columns: self.columns,
            },
            Self {
                cells: bottom,
                columns: self.columns,
            },
        )
    }

    pub(crate) fn new(cells: &'a mut [Cell], columns: usize) -> Self {
        Self { cells, columns }
    }

    fn spanning(&self, rows: usize) -> usize {
        rows.saturating_mul(self.columns).min(self.cells.len())
    }

    fn separate(&mut self, column: usize, row: usize) {
        let Some(position) = self.position(column, row) else {
            return;
        };

        match self.cells[position].columns() {
            2 => self.cells[position + 1] = Cell::BLANK,
            0 => self.cells[position - 1] = Cell::BLANK,
            _ => {}
        }
    }

    const fn position(&self, column: usize, row: usize) -> Option<usize> {
        if column >= self.columns || row >= self.rows() {
            return None;
        }
        Some(row * self.columns + column)
    }
}
