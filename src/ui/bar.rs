//! The brackets and the glyphs a bar is drawn from.
//!
//! Two widgets draw a bar and they are not one widget: the input meter draws a
//! level with the recent peak marked on it, and the looper draws how far
//! through the loop the playhead has reached. What they share is what a bar is
//! made of, so that two of them on one screen are the same bar to look at, and
//! each keeps deciding for itself what goes in a cell.

const OPEN: char = '[';
const CLOSE: char = ']';

pub(crate) const BRACKETS: usize = 2;
pub(crate) const FILLED: char = '#';
pub(crate) const UNFILLED: char = '-';

pub(crate) fn bracketed(cells: usize, glyph: impl Fn(usize) -> char) -> String {
    let mut drawn = String::with_capacity(cells + BRACKETS);

    drawn.push(OPEN);
    drawn.extend((0..cells).map(glyph));
    drawn.push(CLOSE);

    drawn
}
