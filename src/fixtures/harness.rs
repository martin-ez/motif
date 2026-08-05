//! Running a candidate over the whole fixture set and reporting what it scored.
//!
//! [`Score`] measures one sequence against one annotation; this walks the set,
//! applies it to every fixture, and aggregates. It exists so that an accuracy
//! claim in a pull request is a number produced the same way as the number it
//! is compared against — an analyser that loads and aggregates for itself will
//! eventually disagree with another about what the figure means.
//!
//! A candidate arrives as a sequence of timestamps, so nothing here knows about
//! analysers or audio.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Annotation, AnnotationError, Score};

const ANNOTATION_EXTENSION: &str = "beats";

/// Which of a fixture's annotated positions a run is measured against.
///
/// A downbeat is a beat as well, so [`Target::Beats`] scores against every
/// annotated position and [`Target::Downbeats`] against the subset that begins
/// a bar. Scoring one against the other is what the pair is for: a tracker that
/// finds the pulse but not the bar scores well on the first and badly on the
/// second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Every annotated beat.
    Beats,
    /// Only the beats that begin a bar.
    Downbeats,
}

impl Target {
    /// The positions in `annotation` this target is measured against.
    ///
    /// ```
    /// use motif::fixtures::harness::Target;
    /// use motif::fixtures::Annotation;
    ///
    /// let annotation: Annotation = "0.0 downbeat\n0.5 beat\n1.0 downbeat\n".parse()?;
    ///
    /// assert_eq!(Target::Beats.positions(&annotation).count(), 3);
    /// assert_eq!(Target::Downbeats.positions(&annotation).count(), 2);
    /// # Ok::<(), motif::fixtures::AnnotationError>(())
    /// ```
    pub fn positions<'a>(&self, annotation: &'a Annotation) -> impl Iterator<Item = Duration> + 'a {
        let only_downbeats = matches!(self, Self::Downbeats);
        annotation
            .beats()
            .iter()
            .filter(move |beat| beat.is_downbeat || !only_downbeats)
            .map(|beat| beat.at)
    }
}

/// One fixture as the harness loaded it: what it is called, and what it
/// annotates.
///
/// This is what a candidate is asked about. The name is the fixture's file stem,
/// which is also the stem of the audio beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundTruth {
    name: String,
    annotation: Annotation,
}

impl GroundTruth {
    /// What the fixture is called.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where its beats fall, and which of them begin a bar.
    pub fn annotation(&self) -> &Annotation {
        &self.annotation
    }
}

/// Where the fixture set committed to this repository lives.
///
/// Callers go through this rather than each rebuilding the path, so that moving
/// the set is one edit rather than a search.
pub fn checked_in() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Score `candidate` against every fixture in `directory`.
///
/// A fixture is a `.beats` file; anything else is left alone. `candidate` is
/// asked once per fixture and answers with the positions it found, so the
/// harness never learns what produced them.
///
/// # Errors
///
/// Returns [`RunError`] naming what was at fault, and scores nothing. A fixture
/// that cannot be read is not skipped: dropping one raises the aggregate over
/// those that remain, which reads as an improvement.
///
/// ```
/// use motif::fixtures::harness::{self, Target};
///
/// let report = harness::measure(&harness::checked_in(), Target::Beats, |truth| {
///     Target::Beats.positions(truth.annotation()).collect()
/// })?;
///
/// assert_eq!(report.mean_f1(), 1.0);
/// # Ok::<(), harness::RunError>(())
/// ```
pub fn measure(
    directory: &Path,
    target: Target,
    mut candidate: impl FnMut(&GroundTruth) -> Vec<Duration>,
) -> Result<Report, RunError> {
    let set = load(directory)?;

    let rows = set
        .into_iter()
        .map(|truth| {
            let annotated: Vec<Duration> = target.positions(truth.annotation()).collect();
            let detected = candidate(&truth);

            Row {
                name: truth.name,
                score: Score::of(&annotated, &detected),
            }
        })
        .collect();

    Ok(Report { rows })
}

fn load(directory: &Path) -> Result<Vec<GroundTruth>, RunError> {
    let entries = fs::read_dir(directory).map_err(|error| RunError::Directory {
        path: directory.to_owned(),
        error,
    })?;

    let mut set = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| RunError::Directory {
                path: directory.to_owned(),
                error,
            })?
            .path();

        let Some(name) = fixture_name(&path) else {
            continue;
        };

        let text = fs::read_to_string(&path).map_err(|error| RunError::Unreadable {
            fixture: name.clone(),
            error,
        })?;
        let annotation = text.parse().map_err(|error| RunError::Annotation {
            fixture: name.clone(),
            error,
        })?;

        set.push(GroundTruth { name, annotation });
    }

    if set.is_empty() {
        return Err(RunError::Empty {
            path: directory.to_owned(),
        });
    }

    set.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(set)
}

fn fixture_name(path: &Path) -> Option<String> {
    if path.extension()? != OsStr::new(ANNOTATION_EXTENSION) {
        return None;
    }

    Some(path.file_stem()?.to_string_lossy().into_owned())
}

/// What a candidate scored over a fixture set.
///
/// The mean is the figure to quote; the rows are what make a change in it
/// diagnosable, since a mean that drops says something broke and the row that
/// moved says what.
///
/// Rows are ordered by fixture name, so two reports are diffable.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    rows: Vec<Row>,
}

impl Report {
    /// What the candidate scored on each fixture, ordered by name.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The mean of the per-fixture F1 scores, from zero to one.
    ///
    /// Every fixture counts once, whatever its length. The alternative — pooling
    /// the counts across the set — weights the reported figure by how many beats
    /// a fixture happens to contain, which lets a long easy fixture hide a short
    /// hard one.
    pub fn mean_f1(&self) -> f64 {
        if self.rows.is_empty() {
            return 0.0;
        }

        self.rows.iter().map(|row| row.score.f1()).sum::<f64>() / self.rows.len() as f64
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .rows
            .iter()
            .map(|row| row.name.len())
            .max()
            .unwrap_or_default();

        for Row { name, score } in &self.rows {
            writeln!(
                f,
                "{name:<width$}  F1 {:.3}  precision {:.3}  recall {:.3}  hits {}/{}  detected {}",
                score.f1(),
                score.precision(),
                score.recall(),
                score.hits(),
                score.annotated(),
                score.detected(),
            )?;
        }

        write!(
            f,
            "mean F1 {:.3} over {} fixtures",
            self.mean_f1(),
            self.rows.len()
        )
    }
}

/// What a candidate scored on one fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    name: String,
    score: Score,
}

impl Row {
    /// The fixture this scored against.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the candidate scored on it.
    pub fn score(&self) -> Score {
        self.score
    }
}

/// Why a run over a fixture set could not produce a report.
///
/// Every variant fails the whole run rather than dropping one fixture from it,
/// which is the point: a skipped fixture silently raises the aggregate.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    /// The directory holding the set could not be walked.
    Directory {
        /// The directory it tried to walk.
        path: PathBuf,
        /// What the filesystem said.
        error: io::Error,
    },
    /// A fixture's ground truth could not be read off disk.
    Unreadable {
        /// The fixture it belongs to.
        fixture: String,
        /// What the filesystem said.
        error: io::Error,
    },
    /// A fixture's ground truth was read but could not be parsed.
    Annotation {
        /// The fixture it belongs to.
        fixture: String,
        /// What was wrong with it, and where.
        error: AnnotationError,
    },
    /// The directory held no fixtures.
    ///
    /// A run over nothing reports an aggregate over nothing, so it is a failure
    /// rather than a perfect or an empty score.
    Empty {
        /// The directory it walked.
        path: PathBuf,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory { path, error } => {
                write!(f, "{}: {error}", path.display())
            }
            Self::Unreadable { fixture, error } => write!(f, "{fixture}: {error}"),
            Self::Annotation { fixture, error } => write!(f, "{fixture}: {error}"),
            Self::Empty { path } => write!(f, "{}: no fixtures", path.display()),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Directory { error, .. } | Self::Unreadable { error, .. } => Some(error),
            Self::Annotation { error, .. } => Some(error),
            Self::Empty { .. } => None,
        }
    }
}
