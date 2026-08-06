//! Scoring a candidate sequence of positions, or of notes, against a fixture's
//! ground truth.
//!
//! Nothing here knows about audio, fixtures or analysers: two sequences go in
//! and three numbers come out, which is what lets an accuracy claim be checked
//! against sequences written by hand.

use std::fmt;
use std::time::Duration;

use super::Note;
use super::harness::Measured;

/// How well a candidate sequence matches an annotated one, whether of positions
/// or of notes.
///
/// Positions are timestamps from the start of the audio, so this scores beats
/// against beats or downbeats against downbeats — it never learns which.
///
/// The counts are reported alongside the rates because a rate over a handful of
/// positions and the same rate over a thousand are not the same evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    hits: usize,
    annotated: usize,
    detected: usize,
}

impl Score {
    /// How far from an annotated position a candidate may fall and still count
    /// as having found it.
    ///
    /// 70 ms either side is the standard beat-tracking convention, as used by
    /// the MIREX beat-tracking F-measure and `mir_eval` after it. It is a
    /// window, not a precision target: a tighter one measures a detector's
    /// jitter rather than whether it found the beat, and published figures
    /// scored against a different window are not comparable.
    pub const TOLERANCE: Duration = Duration::from_millis(70);

    /// Score the positions in `detected` against the ground truth in
    /// `annotated`.
    ///
    /// A candidate within [`TOLERANCE`](Self::TOLERANCE) of an annotated
    /// position is a hit, and the match is one-to-one: without that, a detector
    /// emitting beats at double rate scores as perfect.
    ///
    /// Each annotated position takes the nearest candidate still free, in the
    /// order given. That is greedy rather than optimal, so it scores at or below
    /// the optimum, never above it.
    ///
    /// ```
    /// use motif::fixtures::Score;
    /// use std::time::Duration;
    ///
    /// let annotated = [Duration::ZERO, Duration::from_millis(500)];
    /// let detected = [Duration::from_millis(20), Duration::from_millis(800)];
    ///
    /// let score = Score::of(&annotated, &detected);
    ///
    /// assert_eq!(score.hits(), 1);
    /// assert_eq!(score.precision(), 0.5);
    /// ```
    pub fn of(annotated: &[Duration], detected: &[Duration]) -> Self {
        let mut taken = vec![false; detected.len()];
        let mut hits = 0;

        for position in annotated {
            if let Some(index) = nearest_free(detected, &taken, *position) {
                taken[index] = true;
                hits += 1;
            }
        }

        Self {
            hits,
            annotated: annotated.len(),
            detected: detected.len(),
        }
    }

    /// How far a note's onset may fall from the annotated one, and the floor
    /// under how far its offset may.
    ///
    /// 50 ms either side, with the offset allowed a fifth of the annotated
    /// note's length where that is wider, as `mir_eval.transcription` scores
    /// note events. The window at the end widens with the note because a long
    /// note's release is not placed as sharply as its attack.
    pub const NOTE_TOLERANCE: Duration = Duration::from_millis(50);

    /// Score the notes in `detected` against the ground truth in `annotated`.
    ///
    /// A note is a hit only where all three of it agree: the pitch exactly, the
    /// onset within [`NOTE_TOLERANCE`](Self::NOTE_TOLERANCE), and the offset
    /// within that or a fifth of the annotated note, whichever is wider.
    /// Matching is one-to-one and greedy, as in [`of`](Self::of).
    ///
    /// ```
    /// use motif::fixtures::{Note, Score};
    /// use std::time::Duration;
    ///
    /// let played = |pitch, onset, offset| Note {
    ///     pitch,
    ///     onset: Duration::from_millis(onset),
    ///     offset: Duration::from_millis(offset),
    /// };
    /// let annotated = [played(60, 0, 400)];
    ///
    /// assert_eq!(Score::of_notes(&annotated, &[played(60, 20, 420)]).hits(), 1);
    /// assert_eq!(Score::of_notes(&annotated, &[played(61, 0, 400)]).hits(), 0);
    /// ```
    pub fn of_notes(annotated: &[Note], detected: &[Note]) -> Self {
        let mut taken = vec![false; detected.len()];
        let mut hits = 0;

        for note in annotated {
            if let Some(index) = nearest_free_note(detected, &taken, note) {
                taken[index] = true;
                hits += 1;
            }
        }

        Self {
            hits,
            annotated: annotated.len(),
            detected: detected.len(),
        }
    }

    /// How many annotated positions the candidate found.
    pub fn hits(&self) -> usize {
        self.hits
    }

    /// How many positions the ground truth annotates.
    pub fn annotated(&self) -> usize {
        self.annotated
    }

    /// How many positions the candidate reported.
    pub fn detected(&self) -> usize {
        self.detected
    }

    /// The share of reported positions that hit, from zero to one.
    ///
    /// A candidate reporting nothing has none, so it scores zero rather than
    /// dividing by nothing.
    pub fn precision(&self) -> f64 {
        share(self.hits, self.detected)
    }

    /// The share of annotated positions that were found, from zero to one.
    ///
    /// Ground truth annotating nothing is scored zero, on the same grounds as
    /// [`precision`](Self::precision).
    pub fn recall(&self) -> f64 {
        share(self.hits, self.annotated)
    }

    /// The harmonic mean of precision and recall, from zero to one.
    ///
    /// This is the single figure to quote: precision alone rewards a detector
    /// that reports one beat it is sure of, and recall alone rewards one that
    /// reports every instant.
    ///
    /// The harmonic mean of the two rates reduces to twice the hits over the
    /// two counts together, which is what this computes: one division instead
    /// of three, and a score of zero where there is nothing on either side
    /// falls out of it rather than needing a case of its own.
    pub fn f1(&self) -> f64 {
        share(2 * self.hits, self.annotated + self.detected)
    }
}

impl Measured for Score {
    const QUOTED: &'static str = "F1";

    fn quoted(&self) -> f64 {
        self.f1()
    }
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "F1 {:.3}  precision {:.3}  recall {:.3}  hits {}/{}  detected {}",
            self.f1(),
            self.precision(),
            self.recall(),
            self.hits,
            self.annotated,
            self.detected,
        )
    }
}

const OFFSET_SHARE: u32 = 5;

fn nearest_free_note(detected: &[Note], taken: &[bool], note: &Note) -> Option<usize> {
    let released = note.offset.saturating_sub(note.onset) / OFFSET_SHARE;
    let release = released.max(Score::NOTE_TOLERANCE);

    detected
        .iter()
        .enumerate()
        .filter(|(index, _)| !taken[*index])
        .filter(|(_, heard)| heard.pitch == note.pitch)
        .filter(|(_, heard)| heard.offset.abs_diff(note.offset) <= release)
        .map(|(index, heard)| (index, heard.onset.abs_diff(note.onset)))
        .filter(|(_, gap)| *gap <= Score::NOTE_TOLERANCE)
        .min_by_key(|(_, gap)| *gap)
        .map(|(index, _)| index)
}

fn nearest_free(detected: &[Duration], taken: &[bool], position: Duration) -> Option<usize> {
    detected
        .iter()
        .enumerate()
        .filter(|(index, _)| !taken[*index])
        .map(|(index, at)| (index, at.abs_diff(position)))
        .filter(|(_, gap)| *gap <= Score::TOLERANCE)
        .min_by_key(|(_, gap)| *gap)
        .map(|(index, _)| index)
}

fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }

    part as f64 / whole as f64
}
