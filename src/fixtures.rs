//! Ground truth for a fixture: where the beats fall, which of them begin a bar,
//! what harmony sounds over them, what a monophonic line plays, and how close a
//! candidate came to finding any of it.
//!
//! A wrong annotation is a silent source of wrong accuracy numbers, so the
//! format is line-oriented text meant to be read and corrected in a pull
//! request rather than taken on trust. [`Score`] and [`Agreement`] are what turn
//! an accuracy claim into a number a reviewer can check, and [`harness`] is what
//! runs one over the whole set.
//!
//! [`Recipe`] runs the other way: the parameters a fixture was rendered from,
//! which is what lets a report say which kind of fixture a candidate lost on
//! rather than only that it lost.

pub mod harness;
pub mod synth;

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

mod chord;
mod recipe;
mod scoring;

pub use chord::{Agreement, Chord, ChordLabel, Comparison, PitchClass, Quality};
pub use recipe::{Axis, Drift, Recipe, Texture};
pub use scoring::Score;

/// One annotated beat.
///
/// # Format
///
/// One entry per line, `<seconds> beat` or `<seconds> downbeat`. A downbeat
/// counts as both kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// Where it falls, measured from the start of the audio.
    pub at: Duration,
    /// Whether it begins a bar.
    pub is_downbeat: bool,
}

/// One annotated note of a monophonic line.
///
/// # Format
///
/// One entry per line, `<seconds> note <pitch> <seconds>`: where it starts, its
/// MIDI note number, and where it stops. A note ends after it starts, and the
/// line is monophonic, so the next one may not start before this one ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// Which note it plays, as a MIDI note number.
    pub pitch: u8,
    /// Where it starts, measured from the start of the audio.
    pub onset: Duration,
    /// Where it stops.
    pub offset: Duration,
}

/// A fixture's ground truth: the beats, the harmony over them, and the notes of
/// a monophonic line.
///
/// # Format
///
/// One entry per line — decimal seconds, then `beat`, `downbeat`, `chord` or
/// `note`, then what that kind carries, which [`Beat`], [`Chord`] and [`Note`]
/// each give. `#` comments a line, and it still counts towards the one an
/// [`AnnotationError`] blames. Timestamps strictly increase within a kind, and
/// at least one beat is annotated and one of those is a downbeat.
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
/// 0.0 chord C:maj
/// 2.0 chord N
/// "
/// .parse()?;
///
/// assert_eq!(annotation.beats().len(), 5);
/// assert_eq!(
///     annotation.downbeats().collect::<Vec<_>>(),
///     [Duration::ZERO, Duration::from_secs(2)]
/// );
/// assert_eq!(annotation.chords()[0].to, Duration::from_secs(2));
/// # Ok::<(), motif::fixtures::AnnotationError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    beats: Vec<Beat>,
    chords: Vec<Chord>,
    notes: Vec<Note>,
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

    /// Every annotated chord, in the order they sound, each spanning up to the
    /// one after it.
    pub fn chords(&self) -> &[Chord] {
        &self.chords
    }

    /// Every annotated note, in the order they are played.
    pub fn notes(&self) -> &[Note] {
        &self.notes
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
        let mut changes: Vec<Change> = Vec::new();
        let mut notes: Vec<Note> = Vec::new();

        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let content = line.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }

            match parse_entry(content, number)? {
                Entry::Beat(beat) => {
                    if beats.last().is_some_and(|previous| beat.at <= previous.at) {
                        return Err(AnnotationError::OutOfOrder { line: number });
                    }
                    beats.push(beat);
                }
                Entry::Change(change) => {
                    if changes
                        .last()
                        .is_some_and(|previous| change.at <= previous.at)
                    {
                        return Err(AnnotationError::OutOfOrder { line: number });
                    }
                    changes.push(change);
                }
                Entry::Note(note) => {
                    check_monophonic(notes.last(), &note, number)?;
                    notes.push(note);
                }
            }
        }

        if beats.is_empty() {
            return Err(AnnotationError::Empty);
        }
        if !beats.iter().any(|beat| beat.is_downbeat) {
            return Err(AnnotationError::NoDownbeats);
        }

        Ok(Self {
            beats,
            chords: spans(&changes)?,
            notes,
        })
    }
}

struct Change {
    at: Duration,
    label: ChordLabel,
}

enum Entry {
    Beat(Beat),
    Change(Change),
    Note(Note),
}

fn spans(changes: &[Change]) -> Result<Vec<Chord>, AnnotationError> {
    if changes
        .last()
        .is_some_and(|last| last.label != ChordLabel::Silent)
    {
        return Err(AnnotationError::UnterminatedChords);
    }

    Ok(changes
        .windows(2)
        .map(|pair| Chord {
            label: pair[0].label,
            from: pair[0].at,
            to: pair[1].at,
        })
        .collect())
}

fn check_monophonic(
    previous: Option<&Note>,
    note: &Note,
    line: usize,
) -> Result<(), AnnotationError> {
    if note.offset <= note.onset {
        return Err(AnnotationError::NoteSpan { line });
    }

    let Some(previous) = previous else {
        return Ok(());
    };
    if note.onset <= previous.onset {
        return Err(AnnotationError::OutOfOrder { line });
    }
    if note.onset < previous.offset {
        return Err(AnnotationError::Overlap { line });
    }

    Ok(())
}

fn parse_entry(content: &str, line: usize) -> Result<Entry, AnnotationError> {
    let mut fields = content.split_whitespace();
    let (Some(timestamp), Some(kind)) = (fields.next(), fields.next()) else {
        return Err(AnnotationError::Malformed { line });
    };
    let at = parse_timestamp(timestamp, line)?;

    match kind {
        "beat" | "downbeat" => end_of_entry(fields, line).map(|()| {
            Entry::Beat(Beat {
                at,
                is_downbeat: kind == "downbeat",
            })
        }),
        "chord" => {
            let Some(spelled) = fields.next() else {
                return Err(AnnotationError::Malformed { line });
            };
            let label = ChordLabel::parse(spelled).ok_or(AnnotationError::ChordLabel { line })?;
            end_of_entry(fields, line).map(|()| Entry::Change(Change { at, label }))
        }
        "note" => {
            let (Some(spelled), Some(end)) = (fields.next(), fields.next()) else {
                return Err(AnnotationError::Malformed { line });
            };
            let pitch = parse_pitch(spelled, line)?;
            let offset = parse_timestamp(end, line)?;
            end_of_entry(fields, line).map(|()| {
                Entry::Note(Note {
                    pitch,
                    onset: at,
                    offset,
                })
            })
        }
        _ => Err(AnnotationError::EntryKind { line }),
    }
}

fn end_of_entry<'a>(
    mut fields: impl Iterator<Item = &'a str>,
    line: usize,
) -> Result<(), AnnotationError> {
    match fields.next() {
        Some(_) => Err(AnnotationError::Malformed { line }),
        None => Ok(()),
    }
}

fn parse_timestamp(field: &str, line: usize) -> Result<Duration, AnnotationError> {
    field
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_sign_positive())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or(AnnotationError::Timestamp { line })
}

const HIGHEST_PITCH: u8 = 127;

fn parse_pitch(field: &str, line: usize) -> Result<u8, AnnotationError> {
    field
        .parse::<u8>()
        .ok()
        .filter(|pitch| *pitch <= HIGHEST_PITCH)
        .ok_or(AnnotationError::Pitch { line })
}

/// Why a fixture's ground truth could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnnotationError {
    /// A line held the wrong number of fields for the kind it named.
    Malformed {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A timestamp was not a number of seconds a beat can fall at.
    Timestamp {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// An entry kind was none of `beat`, `downbeat`, `chord` or `note`.
    EntryKind {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A chord label was not one this vocabulary spells.
    ChordLabel {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A pitch was not a MIDI note number.
    Pitch {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A note did not end after it started.
    NoteSpan {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A note started before the one before it ended.
    ///
    /// The line a transcription fixture annotates is monophonic, so two notes
    /// sounding at once means the annotation is wrong rather than that the
    /// music is polyphonic.
    Overlap {
        /// The line it was on, counted from one.
        line: usize,
    },
    /// A timestamp did not come after the one before it of the same kind.
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
    /// Chords were annotated but the last entry names one.
    ///
    /// A chord runs until the next entry, so the last one has no end until an
    /// `N` says where the harmony stops.
    UnterminatedChords,
}

impl AnnotationError {
    /// The line the annotation failed on, counted from one, where the failure
    /// is attributable to one.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::Malformed { line }
            | Self::Timestamp { line }
            | Self::EntryKind { line }
            | Self::ChordLabel { line }
            | Self::Pitch { line }
            | Self::NoteSpan { line }
            | Self::Overlap { line }
            | Self::OutOfOrder { line } => Some(*line),
            Self::Empty | Self::NoDownbeats | Self::UnterminatedChords => None,
        }
    }
}

impl fmt::Display for AnnotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, described) = match self {
            Self::Malformed { line } => (line, "the entry does not carry what its kind takes"),
            Self::Timestamp { line } => (line, "the timestamp is not a number of seconds"),
            Self::EntryKind { line } => (
                line,
                "the entry kind is not one of 'beat', 'downbeat', 'chord' or 'note'",
            ),
            Self::ChordLabel { line } => {
                (line, "the chord label is not one this vocabulary spells")
            }
            Self::Pitch { line } => (line, "the pitch is not a MIDI note number"),
            Self::NoteSpan { line } => (line, "the note does not end after it starts"),
            Self::Overlap { line } => (line, "the note starts before the one before it ended"),
            Self::OutOfOrder { line } => (
                line,
                "the timestamp does not come after the one before it of its kind",
            ),
            Self::Empty => return f.write_str("the annotation has no beats"),
            Self::NoDownbeats => return f.write_str("the annotation has no downbeats"),
            Self::UnterminatedChords => {
                return f.write_str("the chord entries do not end with 'N'");
            }
        };
        write!(f, "line {line}: {described}")
    }
}

impl std::error::Error for AnnotationError {}
