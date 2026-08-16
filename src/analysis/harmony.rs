//! What harmony a take held, heard over the beats it was played on.
//!
//! Crude by design, and the first thing in the crate to produce a candidate the
//! [`Agreement`](crate::fixtures::Agreement) scorer can grade. One window of
//! [`Chroma`] to the beat, the chord it sits nearest, and neighbouring beats
//! that agree merged into one span — since harmony changes on the beat, a grid
//! is the only segmentation this needs.
//!
//! The samples arrive as a slice rather than as the iterator the envelope and
//! the tracker read, because a beat is a window into the middle of a take and
//! not a pass over the whole of it.

use std::time::Duration;

use crate::fixtures::{Chord, ChordLabel};

use super::{Chroma, Transform};

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Hear the harmony of `samples` over the spans `beats` cuts them into, timed
/// against a clock of `sample_rate` frames a second.
///
/// One span per run of beats sharing a label, ending where the next begins and
/// the last of them at the end of the take. Beats a take does not reach are
/// heard as silence rather than dropped, so a grid placed past the audio is
/// answered rather than ignored.
///
/// ```
/// use motif::analysis::chords;
/// use std::time::Duration;
///
/// let silence = [0.0; 8_000];
///
/// assert_eq!(chords(&silence, 8_000, &[]), []);
/// ```
pub fn chords(samples: &[f32], sample_rate: u32, beats: &[Duration]) -> Vec<Chord> {
    let Some(transform) = Transform::of(Chroma::window(sample_rate)) else {
        return Vec::new();
    };
    let heard: Vec<ChordLabel> = beats
        .iter()
        .map(|beat| over(samples, sample_rate, &transform, *beat))
        .collect();

    spans(&heard, beats, ends(samples, sample_rate, beats))
}

fn over(samples: &[f32], sample_rate: u32, transform: &Transform, beat: Duration) -> ChordLabel {
    let window = framed(samples, transform.window(), frame_of(beat, sample_rate));
    let Some(magnitudes) = transform.magnitudes(&window) else {
        return ChordLabel::Silent;
    };

    Chroma::of(&magnitudes, sample_rate).nearest()
}

fn framed(samples: &[f32], window: usize, start: usize) -> Vec<f32> {
    (0..window)
        .map(|offset| samples.get(start + offset).copied().unwrap_or_default())
        .collect()
}

fn spans(heard: &[ChordLabel], beats: &[Duration], ends: Duration) -> Vec<Chord> {
    let mut changes: Vec<(ChordLabel, Duration)> = Vec::new();

    for (label, from) in heard.iter().zip(beats) {
        if changes.last().is_none_or(|(held, _at)| held != label) {
            changes.push((*label, *from));
        }
    }

    changes
        .iter()
        .enumerate()
        .map(|(index, (label, from))| Chord {
            label: *label,
            from: *from,
            to: changes.get(index + 1).map_or(ends, |(_next, at)| *at),
        })
        .collect()
}

fn ends(samples: &[f32], sample_rate: u32, beats: &[Duration]) -> Duration {
    let played = span_of(samples.len(), sample_rate);

    beats.last().map_or(played, |last| played.max(*last))
}

fn span_of(frames: usize, sample_rate: u32) -> Duration {
    Duration::from_nanos((frames as u128 * NANOS_PER_SECOND / u128::from(sample_rate)) as u64)
}

fn frame_of(when: Duration, sample_rate: u32) -> usize {
    (when.as_nanos() * u128::from(sample_rate) / NANOS_PER_SECOND) as usize
}
