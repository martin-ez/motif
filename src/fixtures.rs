//! Ground truth for a fixture: where the beats fall, which of them begin a bar,
//! and how close a candidate came to finding them.
//!
//! A wrong annotation is a silent source of wrong accuracy numbers, so the
//! format is line-oriented text meant to be read and corrected in a pull
//! request rather than taken on trust. [`Score`] is what turns an accuracy
//! claim into a number a reviewer can check, and [`harness`] is what runs one
//! over the whole set.

pub mod harness;
pub mod synth;

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

mod scoring;

pub use scoring::Score;

/// One annotated beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// Where it falls, measured from the start of the audio.
    pub at: Duration,
    /// Whether it begins a bar.
    pub is_downbeat: bool,
}

/// A fixture's ground truth: every beat, in order, with the downbeats marked.
///
/// # Format
///
/// One beat per line: decimal seconds from the start of the audio, then `beat`
/// or `downbeat`. Timestamps strictly increase and are never negative, and a
/// downbeat counts as both kinds. `#` comments a line; blank and comment lines
/// still count towards the line an [`AnnotationError`] reports.
///
/// At least one beat is annotated and one is a downbeat. Positions are stored,
/// never a tempo, which could not express a fixture that drifts.
///
/// ```
/// use motif::fixtures::Annotation;
/// use std::time::Duration;
///
/// let annotation: Annotation = "\
/// ## two bars at 120 BPM
/// 0.0 downbeat
/// 0.5 beat
/// 1.0 beat
/// 1.5 beat
/// 2.0 downbeat
/// "
/// .parse()?;
///
/// assert_eq!(annotation.beats().len(), 5);
/// assert_eq!(
///     annotation.downbeats().collect::<Vec<_>>(),
///     [Duration::ZERO, Duration::from_secs(2)]
/// );
/// # Ok::<(), motif::fixtures::AnnotationError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    beats: Vec<Beat>,
}

impl Annotation {
    /// Every annotated beat, in the order they fall.
    pub fn beats(&self) -> &[Beat] {
        &self.beats
    }

    /// How long the take runs: from its start to the last beat annotated in it.
    ///
    /// The audio may carry on past that beat, so this is at or under the true
    /// length. That is the safe direction, since a deadline taken as a share of
    /// it comes out tighter rather than more generous.
    pub fn span(&self) -> Duration {
        self.beats.last().map_or(Duration::ZERO, |beat| beat.at)
    }

    /// Where each bar begins.
    pub fn downbeats(&self) -> impl Iterator<Item = Duration> + '_ {
        self.beats
            .iter()
            .filter(|beat| beat.is_downbeat)
            .map(|beat| beat.at)
    }
}

impl FromStr for Annotation {
    type Err = AnnotationError;

    /// Read the format described on [`Annotation`].
    ///
    /// # Errors
    ///
    /// Returns [`AnnotationError`] naming the offending line, so that a
    /// mis-annotated fixture is corrected rather than silently scored.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut beats: Vec<Beat> = Vec::new();

        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let content = line.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }

            let beat = parse_beat(content, number)?;
            if beats.last().is_some_and(|previous| beat.at <= previous.at) {
                return Err(AnnotationError::OutOfOrder { line: number });
            }

            beats.push(beat);
        }

        if beats.is_empty() {
            return Err(AnnotationError::Empty);
        }
        if !beats.iter().any(|beat| beat.is_downbeat) {
            return Err(AnnotationError::NoDownbeats);
        }

        Ok(Self { beats })
    }
}

fn parse_beat(content: &str, line: usize) -> Result<Beat, AnnotationError> {
    let mut fields = content.split_whitespace();
    let (Some(timestamp), Some(kind), None) = (fields.next(), fields.next(), fields.next()) else {
        return Err(AnnotationError::Malformed { line });
    };

    let at = timestamp
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_sign_positive())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or(AnnotationError::Timestamp { line })?;

    let is_downbeat = match kind {
        "beat" => false,
        "downbeat" => true,
        _ => return Err(AnnotationError::BeatKind { line }),
    };

    Ok(Beat { at, is_downbeat })
}

/// Why a fixture's ground truth could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnnotationError {
    /// A line held something other than a timestamp and a beat kind.
    Malformed {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A timestamp was not a number of seconds a beat can fall at.
    Timestamp {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A beat kind was neither `beat` nor `downbeat`.
    BeatKind {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A timestamp did not come after the one before it.
    OutOfOrder {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// Nothing was annotated.
    ///
    /// An annotation with no beats is rejected rather than read as an empty
    /// one, because a fixture that scores against nothing raises an aggregate
    /// without failing anything.
    Empty,
    /// Beats were annotated but none of them begins a bar.
    ///
    /// Rejected for the same reason as [`AnnotationError::Empty`], and it is
    /// the likelier slip of the two: the kinds differ by one word, so a file
    /// whose `downbeat` lines were all written `beat` is well formed on every
    /// other count while scoring downbeats against nothing.
    NoDownbeats,
}

impl AnnotationError {
    /// The line the annotation failed on, counted from one, where the failure
    /// is attributable to one.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::Malformed { line }
            | Self::Timestamp { line }
            | Self::BeatKind { line }
            | Self::OutOfOrder { line } => Some(*line),
            Self::Empty | Self::NoDownbeats => None,
        }
    }
}

impl fmt::Display for AnnotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, described) = match self {
            Self::Malformed { line } => (line, "expected a timestamp and a beat kind"),
            Self::Timestamp { line } => (line, "the timestamp is not a number of seconds"),
            Self::BeatKind { line } => (line, "the beat kind is neither 'beat' nor 'downbeat'"),
            Self::OutOfOrder { line } => {
                (line, "the timestamp does not come after the one before it")
            }
            Self::Empty => return f.write_str("the annotation has no beats"),
            Self::NoDownbeats => return f.write_str("the annotation has no downbeats"),
        };
        write!(f, "line {line}: {described}")
    }
}

impl std::error::Error for AnnotationError {}
