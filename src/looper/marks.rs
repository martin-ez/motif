//! What an analyser found, drawn over the loop it was found in.
//!
//! A mark is a frame and a kind, and turning a frame into a column is
//! [`LoopWaveform`]'s arithmetic rather than a second copy of it here: the two
//! are drawn on one axis, so only one of them may own the mapping.
//!
//! Two rows, because a cell holds one glyph and harmony changes on the beat. The
//! grid goes on one row and what sounds over it on the other, so a chord change
//! on a downbeat draws as both rather than as whichever of them was found last.
//! Within a row the stronger of two sharing a column is the one drawn, which a
//! screen narrower than the loop is long makes the ordinary case rather than the
//! corner.

use std::array;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::LoopWaveform;

const DOWNBEAT: char = '┃';
const BEAT: char = '│';
const CHORD_CHANGE: char = '◆';
const ONSET: char = '•';
const BLANK: char = ' ';

const GRID_ROW: usize = 0;
const EVENTS_ROW: usize = 1;

/// Something an analyser found, at one frame of a loop.
///
/// The four fall into two families — the grid, and what sounds over it — which
/// is what decides the row each is drawn on. They are ordered within a family
/// by which of two sharing a column is the one drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    /// A beat of the grid.
    Beat,
    /// The beat a bar begins on.
    Downbeat,
    /// Where a note starts.
    Onset,
    /// Where the harmony changes.
    ChordChange,
}

impl Mark {
    const fn row(self) -> usize {
        match self {
            Self::Beat | Self::Downbeat => GRID_ROW,
            Self::Onset | Self::ChordChange => EVENTS_ROW,
        }
    }

    const fn glyph(self) -> char {
        match self {
            Self::Beat => BEAT,
            Self::Downbeat => DOWNBEAT,
            Self::Onset => ONSET,
            Self::ChordChange => CHORD_CHANGE,
        }
    }
}

/// What analysis found across a loop, as the frames it found it at.
///
/// ```
/// use motif::looper::{LoopMarks, LoopWaveform, Mark};
///
/// let mut waveform = LoopWaveform::EMPTY;
/// waveform.take(0, [0.0; 4]);
///
/// let mut marks = LoopMarks::none();
/// marks.add(2, Mark::Downbeat);
///
/// assert_eq!(marks.drawn(&waveform, 4), ["  ┃ ", "    "]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoopMarks {
    found: Vec<(u64, Mark)>,
}

impl LoopMarks {
    /// How many rows the marks are drawn on.
    pub const ROWS: usize = 2;

    /// Nothing found, which is how a loop reads for as long as the deadline
    /// lasts.
    pub const fn none() -> Self {
        Self { found: Vec::new() }
    }

    /// Add `mark`, found at frame `at` of the loop.
    ///
    /// Frames arrive in whatever order the analyser found them in, and two of
    /// them may be one frame: what a column shows where several reach it is
    /// [`drawn`](Self::drawn)'s to settle rather than the caller's.
    pub fn add(&mut self, at: u64, mark: Mark) {
        self.found.push((at, mark));
    }

    /// The marks drawn `columns` wide over `shape`, the grid first.
    ///
    /// Each lands in the column `shape` draws its frame in, so the two line up
    /// at every length; one past the end of that loop is dropped. Beats and
    /// downbeats fill the first row and chord changes and note onsets the
    /// second, and where several reach one column the strongest shows: a
    /// downbeat over a beat, a chord change over a note onset.
    pub fn drawn(&self, shape: &LoopWaveform, columns: usize) -> [String; Self::ROWS] {
        let mut strongest: [Vec<Option<Mark>>; Self::ROWS] =
            array::from_fn(|_row| vec![None; columns]);

        for &(at, mark) in &self.found {
            let Some(column) = usize::try_from(at)
                .ok()
                .and_then(|frame| shape.column_of(frame, columns))
            else {
                continue;
            };

            if let Some(held) = strongest[mark.row()].get_mut(column) {
                *held = (*held).max(Some(mark));
            }
        }

        strongest.map(|row| {
            row.into_iter()
                .map(|held| held.map_or(BLANK, Mark::glyph))
                .collect()
        })
    }
}

/// Build a marks handoff, and split it into the end an analyst publishes to and
/// the end the page reads.
///
/// Neither end is the audio callback — a take has already crossed off it by the
/// time anything here has marks to carry — so this crossing is free to allocate
/// and to be as long as the analysis is deep.
///
/// ```
/// use motif::looper::{LoopMarks, Mark, marks_handoff};
///
/// let (mut analyst, page) = marks_handoff();
/// let mut found = LoopMarks::none();
/// found.add(2, Mark::Downbeat);
///
/// analyst.publish(found.clone());
///
/// assert_eq!(page.read(), Some(found));
/// assert_eq!(page.read(), None);
/// ```
pub fn marks_handoff() -> (MarksWriter, MarksReader) {
    let (found, drawn) = channel();

    (MarksWriter { found }, MarksReader { drawn })
}

/// The publishing end of a marks handoff, held by whichever thread analyses the
/// loop.
pub struct MarksWriter {
    found: Sender<LoopMarks>,
}

impl MarksWriter {
    /// Hand `marks` to whoever draws them.
    ///
    /// Returns without waiting, and without an answer: an analyst whose page
    /// has gone has nothing to do differently, and a pass whose marks nobody
    /// wants is one already made.
    pub fn publish(&mut self, marks: LoopMarks) {
        let _handed = self.found.send(marks);
    }
}

/// The reading end of a marks handoff, held by whichever thread draws the loop.
pub struct MarksReader {
    drawn: Receiver<LoopMarks>,
}

impl MarksReader {
    /// The newest analysis published since the last look, or nothing where
    /// there has been none.
    ///
    /// Newest rather than next: a page draws what the loop is now, and an
    /// analysis overtaken while nobody was looking describes a take the player
    /// has already recorded over.
    pub fn read(&self) -> Option<LoopMarks> {
        self.drawn.try_iter().last()
    }
}
