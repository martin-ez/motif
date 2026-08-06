//! The chord vocabulary a fixture is annotated in, and how two segmentations of
//! it are compared.
//!
//! A chord is a label over a span, so it is scored by agreement over time rather
//! than by hits within a tolerance: what a chord scorer has to say is the share
//! of a passage labelled correctly. [`Comparison`] is what grades one error
//! against another — a seventh heard as its triad is not the mistake a chord a
//! tritone away is.

use std::fmt;
use std::time::Duration;

use super::harness::Measured;

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

const SILENT: &str = "N";

const QUALITIES: [Quality; 7] = [
    Quality::Maj,
    Quality::Min,
    Quality::Dim,
    Quality::Aug,
    Quality::Maj7,
    Quality::Min7,
    Quality::Dom7,
];

/// One of the twelve pitch classes, counted in semitones above C.
///
/// A root rather than a pitch: `C` names every C, since the octave a chord is
/// voiced in is not part of what it is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchClass(u8);

impl PitchClass {
    /// The pitch class that many semitones above C, wrapping at the octave.
    pub const fn from_semitone(semitone: u8) -> Self {
        Self(semitone % NAMES.len() as u8)
    }

    /// How far above C it is, from zero to eleven semitones.
    pub const fn semitone(self) -> u8 {
        self.0
    }
}

impl fmt::Display for PitchClass {
    /// Spell it with a sharp, so `Db` writes as `C#`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(NAMES[usize::from(self.0)])
    }
}

/// The quality of a chord: which third and fifth it stacks, and which seventh
/// over them, if any.
///
/// The seven a sketch is built from, spelled as MIREX chord annotations spell
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// A major triad, written `maj`.
    Maj,
    /// A minor triad, written `min`.
    Min,
    /// A diminished triad, written `dim`.
    Dim,
    /// An augmented triad, written `aug`.
    Aug,
    /// A major triad under a major seventh, written `maj7`.
    Maj7,
    /// A minor triad under a minor seventh, written `min7`.
    Min7,
    /// A major triad under a minor seventh, written `7`.
    Dom7,
}

/// What sounds over a span: a chord, or nothing.
///
/// Written as MIREX chord annotations are — a root, a colon and a quality, as
/// in `C:maj`, `A:min` or `G:7` — or `N` where no chord sounds. Roots are
/// spelled with sharps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordLabel {
    /// No chord sounds, written `N`.
    Silent,
    /// A chord on a root, of a quality.
    Sounding(PitchClass, Quality),
}

impl ChordLabel {
    /// Read a label written the way [`ChordLabel`] describes.
    ///
    /// `None` where the text is neither `N` nor a root and quality this
    /// vocabulary holds, so a mis-spelled label fails the fixture rather than
    /// scoring against a chord nobody meant.
    ///
    /// ```
    /// use motif::fixtures::ChordLabel;
    ///
    /// assert_eq!(ChordLabel::parse("F#:min7").map(|l| l.to_string()).as_deref(), Some("F#:min7"));
    /// assert_eq!(ChordLabel::parse("F#:sus4"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Self> {
        if text == SILENT {
            return Some(Self::Silent);
        }

        let (root, quality) = text.split_once(':')?;

        Some(Self::Sounding(named_root(root)?, named_quality(quality)?))
    }
}

impl fmt::Display for ChordLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Silent => f.write_str(SILENT),
            Self::Sounding(root, quality) => write!(f, "{root}:{}", spelling(*quality)),
        }
    }
}

fn named_root(text: &str) -> Option<PitchClass> {
    NAMES
        .iter()
        .position(|name| *name == text)
        .map(|semitone| PitchClass(semitone as u8))
}

fn named_quality(text: &str) -> Option<Quality> {
    QUALITIES
        .into_iter()
        .find(|quality| spelling(*quality) == text)
}

fn spelling(quality: Quality) -> &'static str {
    match quality {
        Quality::Maj => "maj",
        Quality::Min => "min",
        Quality::Dim => "dim",
        Quality::Aug => "aug",
        Quality::Maj7 => "maj7",
        Quality::Min7 => "min7",
        Quality::Dom7 => "7",
    }
}

/// One annotated chord: what sounds, and over what span.
///
/// # Format
///
/// One entry per line, `<seconds> chord <label>`, where the label is what
/// [`ChordLabel`] describes. An entry runs until the next one, so the last must
/// be `N`: without a terminator the final span has no end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// What sounds over the span.
    pub label: ChordLabel,
    /// Where it starts, measured from the start of the audio.
    pub from: Duration,
    /// Where it stops, which is where the next entry starts.
    pub to: Duration,
}

/// How much of a chord label two sides must agree on for the time to count.
///
/// The comparison levels `mir_eval` scores chords at, which is what makes one
/// error gradable against another: a seventh heard as its triad keeps its
/// third, and a chord heard a tritone away keeps nothing. Quote the level
/// beside the figure, since accuracies at different levels are not comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// The roots agree, whatever is stacked over them.
    Root,
    /// The roots agree, and the third is major in both or minor in both.
    Thirds,
    /// The roots agree, and so does the whole quality, seventh included.
    Sevenths,
}

impl Comparison {
    /// Whether `detected` counts as having found `annotated` at this level.
    ///
    /// Silence agrees only with silence: naming a chord where the ground truth
    /// names none is wrong at every level, and so is the reverse.
    ///
    /// ```
    /// use motif::fixtures::{ChordLabel, Comparison};
    ///
    /// let annotated = ChordLabel::parse("C:maj7").expect("a chord label");
    /// let detected = ChordLabel::parse("C:min").expect("a chord label");
    ///
    /// assert!(Comparison::Root.agree(annotated, detected));
    /// assert!(!Comparison::Thirds.agree(annotated, detected));
    /// ```
    pub fn agree(self, annotated: ChordLabel, detected: ChordLabel) -> bool {
        match (annotated, detected) {
            (ChordLabel::Silent, ChordLabel::Silent) => true,
            (ChordLabel::Sounding(heard, quality), ChordLabel::Sounding(root, over)) => {
                heard == root && self.qualities_agree(quality, over)
            }
            _ => false,
        }
    }

    fn qualities_agree(self, annotated: Quality, detected: Quality) -> bool {
        match self {
            Self::Root => true,
            Self::Thirds => third(annotated) == third(detected),
            Self::Sevenths => annotated == detected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Third {
    Major,
    Minor,
}

fn third(quality: Quality) -> Third {
    match quality {
        Quality::Maj | Quality::Aug | Quality::Maj7 | Quality::Dom7 => Third::Major,
        Quality::Min | Quality::Dim | Quality::Min7 => Third::Minor,
    }
}

/// How much of an annotated passage a candidate labelled correctly.
///
/// A share of time rather than of chords: a wrong label over a bar costs more
/// than one over a beat, which is the thing a chord scorer has to say and a
/// hit-within-a-tolerance scorer cannot.
///
/// Time the candidate labels outside the ground truth counts for nothing, and
/// annotated time it leaves unlabelled is disagreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Agreement {
    agreed: Duration,
    total: Duration,
}

impl Agreement {
    /// Score the spans in `detected` against the ground truth in `annotated`,
    /// counting time the two agree over at `comparison`.
    ///
    /// A span agrees for as long as an agreeing candidate overlaps it, and
    /// never for longer than the span itself, so candidates laid over each
    /// other cannot score above one.
    ///
    /// ```
    /// use motif::fixtures::{Agreement, Chord, ChordLabel, Comparison};
    /// use std::time::Duration;
    ///
    /// let over = |label, from, to| Chord {
    ///     label: ChordLabel::parse(label).expect("a chord label"),
    ///     from: Duration::from_secs(from),
    ///     to: Duration::from_secs(to),
    /// };
    /// let annotated = [over("C:maj", 0, 3), over("A:min", 3, 4)];
    /// let detected = [over("C:maj", 0, 3), over("F:maj", 3, 4)];
    ///
    /// assert_eq!(Agreement::of(&annotated, &detected, Comparison::Root).accuracy(), 0.75);
    /// ```
    pub fn of(annotated: &[Chord], detected: &[Chord], comparison: Comparison) -> Self {
        let mut agreed = Duration::ZERO;
        let mut total = Duration::ZERO;

        for truth in annotated {
            let span = truth.to.saturating_sub(truth.from);
            let heard: Duration = detected
                .iter()
                .filter(|candidate| comparison.agree(truth.label, candidate.label))
                .map(|candidate| overlap(truth, candidate))
                .sum();

            agreed += heard.min(span);
            total += span;
        }

        Self { agreed, total }
    }

    /// How much of the annotated passage the candidate labelled correctly.
    pub fn agreed(&self) -> Duration {
        self.agreed
    }

    /// How much passage the ground truth covers.
    pub fn total(&self) -> Duration {
        self.total
    }

    /// The share of the annotated passage labelled correctly, from zero to one.
    ///
    /// Ground truth covering nothing scores zero rather than dividing by
    /// nothing.
    pub fn accuracy(&self) -> f64 {
        if self.total.is_zero() {
            return 0.0;
        }

        self.agreed.as_secs_f64() / self.total.as_secs_f64()
    }
}

impl Measured for Agreement {
    const QUOTED: &'static str = "accuracy";

    fn quoted(&self) -> f64 {
        self.accuracy()
    }
}

impl fmt::Display for Agreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "accuracy {:.3}  agreed {:.3}s of {:.3}s",
            self.accuracy(),
            self.agreed.as_secs_f64(),
            self.total.as_secs_f64(),
        )
    }
}

fn overlap(annotated: &Chord, detected: &Chord) -> Duration {
    let from = annotated.from.max(detected.from);
    let to = annotated.to.min(detected.to);

    to.saturating_sub(from)
}
