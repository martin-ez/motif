//! The decibel scale a level is shown and moved on, and the floor under it.
//!
//! One scale under two things: the bar the input meter draws, and the gain the
//! looper's encoder moves. Both convert between decibels and the linear
//! amplitude the samples carry, and both stop at the same floor — so a floor
//! moved in one of them and not the other is a change that looks applied and is
//! half applied.

const DECIBELS_PER_DECADE: f32 = 20.0;
const DECADE: f32 = 10.0;

/// The quietest level the scale reaches, in decibels below full scale.
///
/// The bottom of the range at both ends of it: a bar draws nothing under this
/// and the gain goes no lower. Sixty decibels is the range a hardware meter of
/// this size is conventionally given — it reaches the noise floor of a line
/// input without spending cells no signal will reach.
pub const FLOOR_DBFS: f32 = -60.0;

/// `amplitude` in decibels, with zero at full scale.
///
/// ```
/// use motif::ui::decibels;
///
/// assert_eq!(decibels(1.0), 0.0);
/// ```
pub fn decibels(amplitude: f32) -> f32 {
    DECIBELS_PER_DECADE * amplitude.log10()
}

/// `decibels` as a linear amplitude, with `1.0` at full scale.
///
/// The inverse of [`decibels`], so what one answers the other takes back.
///
/// ```
/// use motif::ui::amplitude;
///
/// assert_eq!(amplitude(0.0), 1.0);
/// ```
pub fn amplitude(decibels: f32) -> f32 {
    DECADE.powf(decibels / DECIBELS_PER_DECADE)
}
