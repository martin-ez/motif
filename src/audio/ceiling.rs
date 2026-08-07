//! Holding a sum inside full scale, without moving what is under it.
//!
//! A loop is a stack of layers summed as it is read, so the sum leaves full
//! scale while every layer in it is still well inside. Something has to give,
//! and the choice is which: scaling by depth drops every layer the moment a new
//! one opens, and clipping colours the loudest moment of the loop hardest.
//!
//! This gives up the top of the scale instead. Below the ceiling the sum is
//! passed through exactly, and above it the rest of the scale is spread over
//! every sum there is, so a layer already recorded never changes level and a
//! stack of any depth still lands inside.
//!
//! Memoryless, so it is the same curve wherever the loop is summed: nothing to
//! prepare, nothing to recover from, and no gain moving under the player.

const FULL_SCALE: f32 = 1.0;
const SILENCE: f32 = 0.0;
const ROOM: f32 = FULL_SCALE - HELD_ABOVE;

/// The loudest sum that is heard exactly as it was summed.
///
/// Three quarters of full scale. It is the level a lone take starts being held
/// under as much as it is the level a stack is, so the cost of a lower ceiling
/// is paid by the player who overdubs nothing: at three quarters a take peaking
/// at the top of the scale comes back about a decibel down, which is under
/// noticing, and a quarter of the scale is left for every stack above it.
pub const HELD_ABOVE: f32 = 0.75;

/// Hold `sample` inside full scale.
///
/// Exact at or below [`HELD_ABOVE`] and curved above it, arriving at full scale
/// only for a sum no addition of layers can reach. The curve meets the straight
/// part at the same slope, so there is no corner to hear as a sum crosses the
/// ceiling, and it is odd, so it holds a waveform's two halves alike.
///
/// A sample that is not a number, or is infinite, is no level at all and is
/// held at silence: the mix is the last place either can be stopped before the
/// device.
///
/// ```
/// use motif::audio::held;
///
/// assert_eq!(held(0.5), 0.5);
/// assert_eq!(held(1.0), 0.875);
/// ```
pub fn held(sample: f32) -> f32 {
    if !sample.is_finite() {
        return SILENCE;
    }

    let over = (sample.abs() - HELD_ABOVE).max(SILENCE);
    let curved = HELD_ABOVE + over * ROOM / (over + ROOM);

    sample.abs().min(curved).copysign(sample)
}
