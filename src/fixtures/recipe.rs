//! What a synthetic fixture was rendered from, and the axes a report bands by.
//!
//! An aggregate says one candidate beat another. What ranks two approaches is
//! where the loser failed, so a fixture carries the [`Recipe`] it came from and
//! [`Axis`] is what lets a report break its aggregate down one parameter at a
//! time.
//!
//! Nothing here renders or scores anything: `synth` builds a fixture from a
//! recipe and `harness` bands a report by an axis, and this is the vocabulary
//! they share.

use std::fmt;

/// How the pulse moves over a fixture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drift {
    /// One tempo from the first beat to the last.
    Steady,
    /// The tempo moves smoothly from the recipe's to this one.
    Ramp {
        /// What the last interval is taken at, in BPM.
        to: f64,
    },
    /// The pulse pushes and pulls against a steady one of the same tempo.
    Rubato {
        /// How far a beat strays from where that steady pulse would put it.
        pull: f64,
    },
}

/// What a fixture sounds over its beats.
///
/// The percussive parameters live on the variant that has them rather than on
/// [`Recipe`], so a pitched fixture records no syncopation rate for nothing to
/// read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Texture {
    /// Clicks: a tick to the beat, and an accent to the bar.
    Percussion {
        /// How sharply a click attacks, one being an instantaneous rise.
        sharpness: f64,
        /// How many onsets a beat's span carries, evenly subdividing it.
        density: usize,
        /// The share of beats that carry no onset at all.
        dropout: f64,
        /// The share of beats whose onsets fall half a subdivision late.
        syncopation: f64,
    },
    /// A chord to the bar, struck on every beat.
    Chords,
    /// A monophonic line.
    Line,
}

/// The parameters one synthetic fixture was rendered from.
///
/// Two fixtures sharing a recipe render the same beats; what a seed varies is
/// the noise under the clicks, so a recipe is what a report can band by and a
/// seed is not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    /// What its first interval is taken at, in BPM.
    pub tempo: f64,
    /// How many beats there are to the bar.
    pub meter: usize,
    /// How many bars it runs.
    pub bars: usize,
    /// How the pulse moves over it.
    pub drift: Drift,
    /// What it sounds over its beats.
    pub texture: Texture,
}

/// One parameter a report can band its aggregate by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// What the fixture is played at.
    Tempo,
    /// How many beats there are to its bar.
    Meter,
    /// Whether its pulse holds, ramps or strays.
    Drift,
    /// How sharply its clicks attack.
    Sharpness,
    /// How many onsets its beats carry.
    Density,
    /// How often a beat carries none.
    Dropout,
    /// How often an onset falls off the beat.
    Syncopation,
}

impl Axis {
    /// Every axis, in the order a report walks them.
    pub const ALL: [Self; 7] = [
        Self::Tempo,
        Self::Meter,
        Self::Drift,
        Self::Sharpness,
        Self::Density,
        Self::Dropout,
        Self::Syncopation,
    ];

    /// What the axis is called, for the line that names a band.
    pub fn named(&self) -> &'static str {
        match self {
            Self::Tempo => "tempo",
            Self::Meter => "meter",
            Self::Drift => "drift",
            Self::Sharpness => "sharpness",
            Self::Density => "density",
            Self::Dropout => "dropout",
            Self::Syncopation => "syncopation",
        }
    }

    /// Which band `recipe` falls in, and [`None`] where the axis does not
    /// describe it.
    ///
    /// A band is named rather than numbered, and the names sort into the order
    /// their levels do, so a report that sorts its bands by name gets them in
    /// the order a reader expects.
    ///
    /// ```
    /// use motif::fixtures::{Axis, Drift, Recipe, Texture};
    ///
    /// let recipe = Recipe {
    ///     tempo: 90.0,
    ///     meter: 3,
    ///     bars: 4,
    ///     drift: Drift::Steady,
    ///     texture: Texture::Chords,
    /// };
    ///
    /// assert_eq!(Axis::Meter.level(&recipe).as_deref(), Some("3/4"));
    /// assert_eq!(Axis::Density.level(&recipe), None);
    /// ```
    pub fn level(&self, recipe: &Recipe) -> Option<String> {
        let Texture::Percussion {
            sharpness,
            density,
            dropout,
            syncopation,
        } = recipe.texture
        else {
            return self.of_the_grid(recipe);
        };

        match self {
            Self::Sharpness => Some(share(sharpness)),
            Self::Density => Some(format!("{density} to the beat")),
            Self::Dropout => Some(share(dropout)),
            Self::Syncopation => Some(share(syncopation)),
            _ => self.of_the_grid(recipe),
        }
    }

    fn of_the_grid(&self, recipe: &Recipe) -> Option<String> {
        match self {
            Self::Tempo => Some(format!("{:>3.0} BPM", recipe.tempo)),
            Self::Meter => Some(format!("{}/4", recipe.meter)),
            Self::Drift => Some(recipe.drift.to_string()),
            _ => None,
        }
    }
}

fn share(of_the_beats: f64) -> String {
    format!("{of_the_beats:.2}")
}

impl fmt::Display for Drift {
    /// Name the kind and not its size, so that two ramps of different reach
    /// land in one band.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Steady => "steady",
            Self::Ramp { .. } => "ramp",
            Self::Rubato { .. } => "rubato",
        })
    }
}
