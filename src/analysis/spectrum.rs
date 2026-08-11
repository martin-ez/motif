//! The Fourier transform the spectral front end reads a window through.
//!
//! Hand-rolled rather than bought. The whole surface is one transform over a
//! window the device profile fixes, it carries no dependency into a build that
//! has to reach `aarch64`, and its output is checkable against its own
//! definition — which is the thing a bought transform would have been trusted
//! for. What that trust buys elsewhere is speed, and the transform is not what
//! the analysis deadline binds on.

use std::f64::consts::PI;

/// A Fourier transform planned for one window length.
///
/// Radix-2 Cooley-Tukey over a power-of-two window. The twiddle factors are
/// planned once, because the front end runs the same window over every frame
/// and a sine computed per butterfly would cost more than the transform.
///
/// Magnitude is all it hands back, so the transform's sense is not fixed:
/// conjugating every bin would leave the output unchanged, and a caller that
/// comes to want phase has to settle the sign first.
///
/// ```
/// use motif::analysis::Transform;
///
/// let transform = Transform::of(8).expect("eight is a power of two");
/// let steady = transform.magnitudes(&[0.5; 8]).expect("a frame of eight");
///
/// assert_eq!(steady.len(), 5);
/// assert!((steady[0] - 4.0).abs() < 1e-6);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    window: usize,
    twiddles: Vec<(f32, f32)>,
}

impl Transform {
    /// Plan a transform over frames of `window` samples.
    ///
    /// `None` unless `window` is a power of two, which is what radix-2 splits
    /// evenly the whole way down.
    pub fn of(window: usize) -> Option<Self> {
        if !window.is_power_of_two() {
            return None;
        }

        Some(Self {
            window,
            twiddles: (0..window / 2).map(|step| twiddle(step, window)).collect(),
        })
    }

    /// How many samples a frame it transforms carries.
    pub fn window(&self) -> usize {
        self.window
    }

    /// The magnitude of each bin of `frame`, from zero to the Nyquist bin.
    ///
    /// Half the window plus one: a real signal's spectrum is conjugate
    /// symmetric, so the bins above Nyquist mirror the ones below and carry
    /// nothing a caller cannot read from them. `None` if `frame` is not the
    /// window this was planned for.
    pub fn magnitudes(&self, frame: &[f32]) -> Option<Vec<f32>> {
        if frame.len() != self.window {
            return None;
        }

        let mut real = bit_reversed(frame);
        let mut imaginary = vec![0.0; self.window];
        let mut span = 2;

        while span <= self.window {
            self.butterflies(span, &mut real, &mut imaginary);
            span *= 2;
        }

        Some(
            (0..=self.nyquist())
                .map(|bin| real[bin].hypot(imaginary[bin]))
                .collect(),
        )
    }

    fn nyquist(&self) -> usize {
        self.twiddles.len()
    }

    fn butterflies(&self, span: usize, real: &mut [f32], imaginary: &mut [f32]) {
        let half = span / 2;
        let stride = self.window / span;

        for start in (0..self.window).step_by(span) {
            for offset in 0..half {
                let (cosine, sine) = self.twiddles[offset * stride];
                let top = start + offset;
                let bottom = top + half;

                let turned_real = real[bottom] * cosine - imaginary[bottom] * sine;
                let turned_imaginary = real[bottom] * sine + imaginary[bottom] * cosine;

                real[bottom] = real[top] - turned_real;
                imaginary[bottom] = imaginary[top] - turned_imaginary;
                real[top] += turned_real;
                imaginary[top] += turned_imaginary;
            }
        }
    }
}

fn twiddle(step: usize, window: usize) -> (f32, f32) {
    let turn = 2.0 * PI * step as f64 / window as f64;

    (turn.cos() as f32, turn.sin() as f32)
}

fn bit_reversed(frame: &[f32]) -> Vec<f32> {
    let bits = frame.len().trailing_zeros();

    (0..frame.len())
        .map(|index| frame[reversing(index, bits)])
        .collect()
}

fn reversing(index: usize, bits: u32) -> usize {
    (0..bits).fold(0, |reversed, bit| (reversed << 1) + ((index >> bit) & 1))
}
