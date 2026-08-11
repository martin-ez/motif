//! The Fourier transform the spectral front end is built on: the magnitude
//! spectrum of a window of samples.
//!
//! The facts worth stating are that it agrees with a transform computed
//! straight from the definition, that a sinusoid sitting on a bin centre lands
//! in that bin and nowhere else, that an impulse is flat across the spectrum
//! wherever it falls, that a constant is all in the zeroth bin, that there is
//! one bin per pair of samples plus one, and that a window or a frame of the
//! wrong length is refused rather than transformed.

use motif::analysis::Transform;
use std::f64::consts::PI;

const WINDOW: usize = 64;
const BINS: usize = 33;
const TOLERANCE: f32 = 1e-3;
const IMPULSE: f32 = 0.7;
const IMPULSE_AT: usize = 3;
const CONSTANT: f32 = 0.25;
const ON_A_BIN: usize = 5;
const AMPLITUDE: f32 = 0.4;

/// The transform of `frame` computed straight from its definition, which is
/// the reference the fast one is checked against.
fn from_the_definition(frame: &[f32]) -> Vec<f32> {
    let length = frame.len();

    (0..=length / 2)
        .map(|bin| {
            let mut real = 0.0;
            let mut imaginary = 0.0;

            for (index, sample) in frame.iter().enumerate() {
                let turn = -2.0 * PI * bin as f64 * index as f64 / length as f64;
                real += f64::from(*sample) * turn.cos();
                imaginary += f64::from(*sample) * turn.sin();
            }

            real.hypot(imaginary) as f32
        })
        .collect()
}

/// A mix with no symmetry to flatter the transform: three partials whose
/// frequencies fall between bins, at falling amplitudes, over a slope.
fn mixed(length: usize) -> Vec<f32> {
    (0..length)
        .map(|index| {
            let position = index as f64 / length as f64;
            let partial = |cycles: f64, level: f64, phase: f64| {
                level * (2.0 * PI * cycles * position + phase).sin()
            };

            (partial(3.5, 0.6, 0.4)
                + partial(7.25, 0.3, 1.1)
                + partial(11.75, 0.15, 2.3)
                + 0.2 * position) as f32
        })
        .collect()
}

fn planned(window: usize) -> Transform {
    Transform::of(window).expect("a power of two is a window the transform accepts")
}

fn spectrum(transform: &Transform, frame: &[f32]) -> Vec<f32> {
    transform
        .magnitudes(frame)
        .expect("a frame of the planned window is one the transform accepts")
}

#[test]
fn magnitudes_match_a_transform_computed_from_the_definition() {
    let frame = mixed(WINDOW);
    let reference = from_the_definition(&frame);

    for (bin, (fast, slow)) in spectrum(&planned(WINDOW), &frame)
        .iter()
        .zip(&reference)
        .enumerate()
    {
        assert!(
            (fast - slow).abs() <= TOLERANCE,
            "bin {bin}: {fast} against {slow} from the definition"
        );
    }
}

#[test]
fn a_sinusoid_on_a_bin_centre_lands_in_that_bin_alone() {
    let frame: Vec<f32> = (0..WINDOW)
        .map(|index| {
            AMPLITUDE * (2.0 * PI * ON_A_BIN as f64 * index as f64 / WINDOW as f64).sin() as f32
        })
        .collect();

    let magnitudes = spectrum(&planned(WINDOW), &frame);
    let expected = AMPLITUDE * WINDOW as f32 / 2.0;

    assert!((magnitudes[ON_A_BIN] - expected).abs() <= TOLERANCE);
    for (bin, magnitude) in magnitudes.iter().enumerate() {
        if bin != ON_A_BIN {
            assert!(*magnitude <= TOLERANCE, "bin {bin} carries {magnitude}");
        }
    }
}

#[test]
fn an_impulse_is_flat_across_every_bin() {
    let mut frame = vec![0.0; WINDOW];
    frame[IMPULSE_AT] = IMPULSE;

    for (bin, magnitude) in spectrum(&planned(WINDOW), &frame).iter().enumerate() {
        assert!(
            (magnitude - IMPULSE).abs() <= TOLERANCE,
            "bin {bin} carries {magnitude} rather than {IMPULSE}"
        );
    }
}

#[test]
fn a_constant_is_all_in_the_zeroth_bin() {
    let magnitudes = spectrum(&planned(WINDOW), &vec![CONSTANT; WINDOW]);

    assert!((magnitudes[0] - CONSTANT * WINDOW as f32).abs() <= TOLERANCE);
    for (bin, magnitude) in magnitudes.iter().enumerate().skip(1) {
        assert!(*magnitude <= TOLERANCE, "bin {bin} carries {magnitude}");
    }
}

#[test]
fn there_is_one_bin_per_pair_of_samples_plus_one() {
    assert_eq!(spectrum(&planned(WINDOW), &mixed(WINDOW)).len(), BINS);
}

#[test]
fn the_window_is_the_one_it_was_planned_for() {
    assert_eq!(planned(WINDOW).window(), WINDOW);
}

#[test]
fn a_window_that_is_not_a_power_of_two_is_refused() {
    assert!(Transform::of(WINDOW - 1).is_none());
    assert!(Transform::of(0).is_none());
    assert!(Transform::of(WINDOW).is_some());
}

#[test]
fn a_frame_the_wrong_length_is_refused() {
    let transform = planned(WINDOW);

    assert!(transform.magnitudes(&mixed(WINDOW - 1)).is_none());
    assert!(transform.magnitudes(&mixed(WINDOW + 1)).is_none());
    assert!(transform.magnitudes(&mixed(WINDOW)).is_some());
}
