//! Twelve pitch classes folded out of a spectrum, and the chord they sit
//! nearest.
//!
//! What a chord is called does not depend on the octave it was voiced in, so
//! the octave is the first thing to throw away: every bin the transform hands
//! back is charged to the pitch class it is nearest, and what is left is twelve
//! numbers.
//!
//! Matching is by correlation against a binary template, one per entry in the
//! vocabulary. Correlation rather than a sum over the chord's own tones,
//! because a sum rewards a template for having more of them and would hear
//! every triad as the seventh that contains it.

use crate::fixtures::{ChordLabel, PitchClass, Quality};

/// How many pitch classes there are, which is how wide a chroma is.
const CLASSES: usize = 12;

/// The lowest pitch the fold counts, as a MIDI note number.
///
/// C3. Below it the semitones are narrower than a window this side of a
/// quarter of a second resolves, and what sounds down there is mostly the body
/// of a drum rather than a chord tone.
const LOWEST: u8 = 48;

/// The highest pitch the fold counts, as a MIDI note number.
///
/// C7, above which what is left is the harmonics of what was played rather
/// than anything anyone voiced.
const HIGHEST: u8 = 96;

const CONCERT_A: f64 = 440.0;
const CONCERT_A_PITCH: f64 = 69.0;
const SEMITONES: f64 = 12.0;

/// How much of each pitch class sounds over one window.
///
/// ```
/// use motif::analysis::Chroma;
/// use motif::fixtures::ChordLabel;
///
/// assert_eq!(Chroma::of(&[0.0; 1025], 8_000).nearest(), ChordLabel::Silent);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chroma {
    weights: [f32; CLASSES],
    pitched: bool,
}

impl Chroma {
    /// Fold `magnitudes`, as [`Transform`](super::Transform) hands them back,
    /// into the twelve classes they belong to.
    ///
    /// The window they came from is the length that many magnitudes describe,
    /// read at `sample_rate`. What falls outside the range the fold counts is
    /// dropped rather than wrapped into a class it does not belong to.
    pub fn of(magnitudes: &[f32], sample_rate: u32) -> Self {
        let mut weights = [0.0; CLASSES];
        let window = window_of(magnitudes.len());
        let at = |bin: usize| bin as f64 * f64::from(sample_rate) / window as f64;

        for (bin, magnitude) in magnitudes.iter().enumerate() {
            if let Some(pitch) = counted(at(bin)) {
                weights[usize::from(pitch % CLASSES as u8)] += magnitude;
            }
        }

        Self {
            weights,
            pitched: loudest(magnitudes).is_some_and(|bin| counted(at(bin)).is_some()),
        }
    }

    /// The window a transform must be planned over for this fold to tell one
    /// semitone from the next at the bottom of the range it counts.
    ///
    /// A power of two, since that is what the transform splits evenly.
    ///
    /// ```
    /// use motif::analysis::Chroma;
    ///
    /// assert_eq!(Chroma::window(8_000), 2_048);
    /// ```
    pub fn window(sample_rate: u32) -> usize {
        let narrowest = hertz(LOWEST + 1) - hertz(LOWEST);

        ((f64::from(sample_rate) / narrowest).ceil() as usize).next_power_of_two()
    }

    /// The chord in the vocabulary these weights sit nearest.
    ///
    /// Silent unless something correlates at half or better: weights carrying
    /// the chord's own classes and nothing else correlate at one, and weights
    /// spread evenly over the twelve — where a window of noise sits — at zero,
    /// so halfway is the line between looking like a chord and looking like
    /// nothing. Silent too where the loudest of the window fell outside the
    /// counted range, since correlation is blind to how much sounded.
    pub fn nearest(&self) -> ChordLabel {
        if !self.pitched {
            return ChordLabel::Silent;
        }
        let heard = centred(&self.weights);

        every_chord()
            .filter_map(|(label, template)| {
                let fit = correlation(&heard, &centred(&template))?;

                (fit >= FIT).then_some((label, fit))
            })
            .max_by(|one, other| one.1.total_cmp(&other.1))
            .map_or(ChordLabel::Silent, |(label, _fit)| label)
    }
}

const FIT: f32 = 0.5;

fn loudest(magnitudes: &[f32]) -> Option<usize> {
    magnitudes
        .iter()
        .enumerate()
        .max_by(|one, other| one.1.total_cmp(other.1))
        .map(|(bin, _weight)| bin)
}

fn window_of(magnitudes: usize) -> usize {
    magnitudes.saturating_sub(1) * 2
}

fn hertz(pitch: u8) -> f64 {
    CONCERT_A * ((f64::from(pitch) - CONCERT_A_PITCH) / SEMITONES).exp2()
}

fn counted(hertz: f64) -> Option<u8> {
    if hertz <= 0.0 {
        return None;
    }
    let pitch = (SEMITONES * (hertz / CONCERT_A).log2() + CONCERT_A_PITCH).round();

    (pitch >= f64::from(LOWEST) && pitch <= f64::from(HIGHEST)).then_some(pitch as u8)
}

fn every_chord() -> impl Iterator<Item = (ChordLabel, [f32; CLASSES])> {
    (0..CLASSES as u8).flat_map(|root| {
        Quality::ALL.into_iter().map(move |quality| {
            (
                ChordLabel::Sounding(PitchClass::from_semitone(root), quality),
                template(root, quality),
            )
        })
    })
}

fn template(root: u8, quality: Quality) -> [f32; CLASSES] {
    let mut tones = [0.0; CLASSES];

    for interval in quality.intervals() {
        tones[usize::from((root + interval) % CLASSES as u8)] = 1.0;
    }

    tones
}

fn centred(weights: &[f32; CLASSES]) -> [f32; CLASSES] {
    let mean = weights.iter().sum::<f32>() / CLASSES as f32;

    weights.map(|weight| weight - mean)
}

fn correlation(heard: &[f32; CLASSES], template: &[f32; CLASSES]) -> Option<f32> {
    let spread = norm(heard) * norm(template);
    if spread == 0.0 {
        return None;
    }
    let together: f32 = heard
        .iter()
        .zip(template)
        .map(|(weight, tone)| weight * tone)
        .sum();

    Some(together / spread)
}

fn norm(weights: &[f32; CLASSES]) -> f32 {
    weights
        .iter()
        .map(|weight| weight * weight)
        .sum::<f32>()
        .sqrt()
}
