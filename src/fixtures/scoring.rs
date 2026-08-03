//! Scoring a candidate sequence of positions against a fixture's ground truth.
//!
//! Nothing here knows about audio, fixtures or analysers: two sequences of
//! timestamps go in and three numbers come out, which is what lets an accuracy
//! claim be checked against sequences written by hand.

use std::time::Duration;

/// How well a candidate sequence of positions matches an annotated one.
///
/// Positions are timestamps from the start of the audio, so this scores beats
/// against beats or downbeats against downbeats — it never learns which.
///
/// The counts are reported alongside the rates because a rate over a handful of
/// positions and the same rate over a thousand are not the same evidence, and
/// only the counts add up across fixtures.
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
    /// position is a hit, and the match is one-to-one: an annotated position
    /// admits one hit and a candidate answers for one annotation. Without that,
    /// a detector emitting beats at double rate scores as perfect.
    ///
    /// Each annotated position takes the nearest candidate still free, working
    /// through `annotated` in the order given. That is greedy rather than an
    /// optimal assignment, so a candidate can be taken by an earlier annotation
    /// that a later one also had a claim on — which scores at or below the
    /// optimum, never above it.
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
    pub fn f1(&self) -> f64 {
        let precision = self.precision();
        let recall = self.recall();
        if precision + recall == 0.0 {
            return 0.0;
        }

        2.0 * precision * recall / (precision + recall)
    }
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
