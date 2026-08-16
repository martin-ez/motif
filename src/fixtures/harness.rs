//! Running a candidate over the whole fixture set and reporting what it scored.
//!
//! [`Score`] and [`Agreement`] measure one candidate against one annotation;
//! this walks the set, applies one of them to every fixture, and aggregates. It
//! exists so that an accuracy claim in a pull request is a number produced the
//! same way as the number it is compared against — an analyser that loads and
//! aggregates for itself will eventually disagree with another about what the
//! figure means.
//!
//! Every fixture is timed as well as scored, against a [`deadline`] taken as a
//! share of the take, so each fixture sets its own.
//!
//! A candidate answers in beats, chords or notes, so nothing here knows what
//! produced them. Off disk it is asked about an annotation alone; over a set
//! rendered in memory it is handed the fixture, audio and all.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::synth::Fixture;
use super::{Agreement, Annotation, AnnotationError, Axis, Beat, Chord, Comparison, Note};
use super::{Recipe, Score};

const ANNOTATION_EXTENSION: &str = "beats";

/// A per-fixture figure the harness can aggregate and name.
///
/// One [`Report`] serves every scorer rather than one report type each, so that
/// a figure quoted in a pull request is produced, aggregated and labelled the
/// same way whatever was measured.
pub trait Measured: Copy + fmt::Display {
    /// What the figure is called, for the line that aggregates it.
    const QUOTED: &'static str;

    /// The single figure to quote for one fixture, from zero to one.
    fn quoted(&self) -> f64;
}

/// The share of a take that analysis has to answer in.
///
/// Half, so the result is up before the loop passes the midpoint of its first
/// replay. A share rather than a fixed figure because the loop length is what
/// the player chose, and it is the wait they are measuring against. It is one
/// value, which keeps what it should be a question the instrument answers
/// rather than a decision spread through the code.
pub const DEADLINE_SHARE: f64 = 0.5;

/// How long analysis has over a take of `length`, measured from the take's last
/// frame to the result reaching the player.
///
/// [`DEADLINE_SHARE`] of the take. The loop wraps to bar one and plays again
/// the moment the take closes, so what bounds the wait is the loop the player
/// set rather than any fixed figure.
///
/// ```
/// use motif::fixtures::harness;
/// use std::time::Duration;
///
/// assert_eq!(harness::deadline(Duration::from_secs(8)), Duration::from_secs(4));
/// ```
pub fn deadline(length: Duration) -> Duration {
    length.mul_f64(DEADLINE_SHARE)
}

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
        self.among(annotation.beats())
    }

    /// The positions among `beats` this target is measured against.
    ///
    /// What [`positions`](Self::positions) reads off an annotation, for a
    /// fixture rendered in memory that never became one.
    pub fn among<'a>(&self, beats: &'a [Beat]) -> impl Iterator<Item = Duration> + 'a {
        let only_downbeats = matches!(self, Self::Downbeats);
        beats
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
/// asked once per fixture and answers with positions, so the harness never
/// learns what produced them; only that call is timed, never the loading.
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
/// assert_eq!(report.mean(), 1.0);
/// # Ok::<(), harness::RunError>(())
/// ```
pub fn measure(
    directory: &Path,
    target: Target,
    mut candidate: impl FnMut(&GroundTruth) -> Vec<Duration>,
) -> Result<Report, RunError> {
    measure_with(
        directory,
        |_| true,
        |truth| {
            let annotated: Vec<Duration> = target.positions(truth.annotation()).collect();
            let (detected, elapsed) = timed(|| candidate(truth));

            (Score::of(&annotated, &detected), elapsed)
        },
    )
}

/// Score `candidate`'s chords against every fixture in `directory` that
/// annotates harmony, agreeing at `comparison`.
///
/// A fixture whose annotation carries no chords is not part of the run at all,
/// rather than scoring zero: a rhythm fixture has no harmony to be wrong about,
/// and counting it would put the figure below what any analyser could reach.
///
/// # Errors
///
/// Returns [`RunError`] as [`measure`] does, and [`RunError::Empty`] where no
/// fixture in the set annotates harmony.
pub fn measure_chords(
    directory: &Path,
    comparison: Comparison,
    mut candidate: impl FnMut(&GroundTruth) -> Vec<Chord>,
) -> Result<Report<Agreement>, RunError> {
    measure_with(
        directory,
        |truth| !truth.annotation().chords().is_empty(),
        |truth| {
            let (detected, elapsed) = timed(|| candidate(truth));

            (
                Agreement::of(truth.annotation().chords(), &detected, comparison),
                elapsed,
            )
        },
    )
}

/// Score `candidate`'s notes against every fixture in `directory` that
/// annotates a monophonic line.
///
/// A fixture whose annotation carries no notes is left out of the run, on the
/// same grounds as in [`measure_chords`].
///
/// # Errors
///
/// Returns [`RunError`] as [`measure`] does, and [`RunError::Empty`] where no
/// fixture in the set annotates a line.
pub fn measure_notes(
    directory: &Path,
    mut candidate: impl FnMut(&GroundTruth) -> Vec<Note>,
) -> Result<Report<Score>, RunError> {
    measure_with(
        directory,
        |truth| !truth.annotation().notes().is_empty(),
        |truth| {
            let (detected, elapsed) = timed(|| candidate(truth));

            (
                Score::of_notes(truth.annotation().notes(), &detected),
                elapsed,
            )
        },
    )
}

/// Score `candidate` against every fixture in `set`, which was rendered rather
/// than read.
///
/// The candidate is handed the fixture itself, so it hears the audio as well as
/// the beats behind it, and each row records the [`Recipe`] its fixture came
/// from so the report can be banded. Nothing is read, so nothing can fail;
/// scoring an empty set reports a mean of zero over no rows.
///
/// ```
/// use motif::fixtures::harness::{self, Target};
/// use motif::fixtures::synth;
///
/// let set = synth::drawn(synth::DEVELOPMENT[0], 2);
/// let report = harness::measure_rendered(&set, Target::Downbeats, |fixture| {
///     Target::Downbeats.among(fixture.beats()).collect()
/// });
///
/// assert_eq!(report.mean(), 1.0);
/// ```
pub fn measure_rendered(
    set: &[Fixture],
    target: Target,
    mut candidate: impl FnMut(&Fixture) -> Vec<Duration>,
) -> Report {
    let rows = set
        .iter()
        .map(|fixture| {
            let annotated: Vec<Duration> = target.among(fixture.beats()).collect();
            let (detected, elapsed) = timed(|| candidate(fixture));

            Row {
                name: fixture.name().to_owned(),
                score: Score::of(&annotated, &detected),
                elapsed,
                deadline: deadline(last_beat(fixture)),
                recipe: Some(*fixture.recipe()),
            }
        })
        .collect();

    Report { rows }
}

/// Score `candidate`'s chords against every fixture in `set` that voices any,
/// agreeing at `comparison`.
///
/// [`measure_rendered`] for harmony: the candidate hears the audio, each row
/// records the [`Recipe`] behind it, and a fixture voicing nothing is left out
/// of the run on the grounds [`measure_chords`] gives. A set voicing nothing at
/// all reports a mean of zero over no rows.
///
/// ```
/// use motif::fixtures::harness;
/// use motif::fixtures::synth;
/// use motif::fixtures::{Comparison, Drift, Recipe, Texture};
///
/// let recipe = Recipe {
///     tempo: 120.0,
///     meter: 4,
///     bars: 4,
///     drift: Drift::Steady,
///     texture: Texture::Chords,
/// };
/// let set = [synth::rendered("voiced", recipe)];
///
/// let report = harness::measure_rendered_chords(&set, Comparison::Sevenths, |fixture| {
///     fixture.chords().to_vec()
/// });
///
/// assert_eq!(report.mean(), 1.0);
/// ```
pub fn measure_rendered_chords(
    set: &[Fixture],
    comparison: Comparison,
    mut candidate: impl FnMut(&Fixture) -> Vec<Chord>,
) -> Report<Agreement> {
    let rows = set
        .iter()
        .filter(|fixture| !fixture.chords().is_empty())
        .map(|fixture| {
            let (detected, elapsed) = timed(|| candidate(fixture));

            Row {
                name: fixture.name().to_owned(),
                score: Agreement::of(fixture.chords(), &detected, comparison),
                elapsed,
                deadline: deadline(last_beat(fixture)),
                recipe: Some(*fixture.recipe()),
            }
        })
        .collect();

    Report { rows }
}

fn last_beat(fixture: &Fixture) -> Duration {
    fixture
        .beats()
        .last()
        .map_or(Duration::ZERO, |beat| beat.at)
}

fn measure_with<S>(
    directory: &Path,
    annotates: impl Fn(&GroundTruth) -> bool,
    mut score: impl FnMut(&GroundTruth) -> (S, Duration),
) -> Result<Report<S>, RunError> {
    let set: Vec<GroundTruth> = load(directory)?.into_iter().filter(annotates).collect();
    if set.is_empty() {
        return Err(RunError::Empty {
            path: directory.to_owned(),
        });
    }

    let rows = set
        .into_iter()
        .map(|truth| {
            let allowed = deadline(truth.annotation().span());
            let (score, elapsed) = score(&truth);

            Row {
                name: truth.name,
                score,
                elapsed,
                deadline: allowed,
                recipe: None,
            }
        })
        .collect();

    Ok(Report { rows })
}

fn timed<T>(produce: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let produced = produce();

    (produced, started.elapsed())
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
/// moved says what. [`by`](Self::by) is the same argument one level up: it says
/// which kind of fixture the candidate lost on rather than which one.
///
/// Rows come in the order the set gave them, which off disk is by name, so two
/// reports are diffable.
#[derive(Debug, Clone, PartialEq)]
pub struct Report<S = Score> {
    rows: Vec<Row<S>>,
}

impl<S: Measured> Report<S> {
    /// What the candidate scored on each fixture, ordered by name.
    pub fn rows(&self) -> &[Row<S>] {
        &self.rows
    }

    /// The mean of the per-fixture figures, from zero to one.
    ///
    /// Every fixture counts once, whatever its length. The alternative — pooling
    /// the counts across the set — weights the reported figure by how many beats
    /// a fixture happens to contain, which lets a long easy fixture hide a short
    /// hard one.
    pub fn mean(&self) -> f64 {
        if self.rows.is_empty() {
            return 0.0;
        }

        self.rows.iter().map(|row| row.score.quoted()).sum::<f64>() / self.rows.len() as f64
    }

    /// What the candidate scored on each level of `axis`, in level order.
    ///
    /// This is what ranks two approaches rather than merely separating them: an
    /// aggregate says one lost, and a band says where. A row recording no
    /// recipe, or one the axis does not describe, is in no band at all — a mean
    /// over fixtures whose parameter is unknown says nothing.
    pub fn by(&self, axis: Axis) -> Vec<Band> {
        let mut bands: Vec<Band> = Vec::new();

        for row in &self.rows {
            let Some(level) = row.recipe.as_ref().and_then(|recipe| axis.level(recipe)) else {
                continue;
            };
            match bands.iter_mut().find(|band| band.level == level) {
                Some(band) => band.take(row.score.quoted()),
                None => bands.push(Band::opened(level, row.score.quoted())),
            }
        }
        bands.sort_by(|one, other| one.level.cmp(&other.level));

        bands
    }

    /// The tightest headroom any fixture left against its own deadline.
    ///
    /// The smallest rather than the mean: a set where one fixture overran did
    /// not meet the deadline, however much room the others left. Zero says one
    /// of them spent everything it had.
    pub fn headroom(&self) -> Duration {
        self.rows
            .iter()
            .map(Row::headroom)
            .min()
            .unwrap_or_default()
    }
}

impl<S: Measured> fmt::Display for Report<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self
            .rows
            .iter()
            .map(|row| row.name.len())
            .max()
            .unwrap_or_default();

        for Row {
            name,
            score,
            elapsed,
            deadline,
            ..
        } in &self.rows
        {
            writeln!(
                f,
                "{name:<width$}  {score}  took {elapsed:.1?} of {deadline:.1?}"
            )?;
        }

        write!(
            f,
            "mean {} {:.3} over {} fixtures, headroom {:.1?}",
            S::QUOTED,
            self.mean(),
            self.rows.len(),
            self.headroom()
        )
    }
}

/// What a candidate scored on one fixture, and what it spent doing it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row<S = Score> {
    name: String,
    score: S,
    elapsed: Duration,
    deadline: Duration,
    recipe: Option<Recipe>,
}

impl<S: Measured> Row<S> {
    /// The fixture this scored against.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the candidate scored on it.
    pub fn score(&self) -> S {
        self.score
    }

    /// How long the candidate took over it.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// How long it had, which is [`DEADLINE_SHARE`] of this fixture's length.
    pub fn deadline(&self) -> Duration {
        self.deadline
    }

    /// What is left of that deadline, and zero where the candidate spent it.
    pub fn headroom(&self) -> Duration {
        self.deadline.saturating_sub(self.elapsed)
    }

    /// What the fixture was rendered from, where that is recorded.
    ///
    /// A fixture read off disk is an annotation and the audio beside it, and
    /// nothing says what produced either, so it records none.
    pub fn recipe(&self) -> Option<&Recipe> {
        self.recipe.as_ref()
    }
}

/// What a candidate scored over the fixtures sharing one level of an axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    level: String,
    total: f64,
    fixtures: usize,
}

impl Band {
    /// Which level of the axis these fixtures share.
    pub fn level(&self) -> &str {
        &self.level
    }

    /// How many fixtures the band covers.
    ///
    /// Reported beside the mean because a mean over two fixtures and the same
    /// mean over twenty are not the same evidence.
    pub fn fixtures(&self) -> usize {
        self.fixtures
    }

    /// The mean of their figures, from zero to one.
    pub fn mean(&self) -> f64 {
        if self.fixtures == 0 {
            return 0.0;
        }

        self.total / self.fixtures as f64
    }

    fn opened(level: String, quoted: f64) -> Self {
        Self {
            level,
            total: quoted,
            fixtures: 1,
        }
    }

    fn take(&mut self, quoted: f64) {
        self.total += quoted;
        self.fixtures += 1;
    }
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}  mean {:.3} over {} fixtures",
            self.level,
            self.mean(),
            self.fixtures
        )
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
    /// The directory held nothing this run could score.
    ///
    /// A run over nothing reports an aggregate over nothing, so it is a failure
    /// rather than a perfect or an empty score. Scoring harmony or a line, a
    /// fixture annotating neither is not in the run, so a set where none does
    /// fails here too.
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
