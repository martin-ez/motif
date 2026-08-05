//! The bar a player watches the input on, and the peak it keeps up afterwards.
//!
//! Decibels rather than the amplitude the samples carry, because a linear bar
//! spends most of its width on the top few dB and crowds everything a player
//! plays at into the first cell or two.
//!
//! The bar follows the RMS and the marker the peak, the two saying different
//! things: a take that clipped and one that did not are the same loudness, and
//! a meter showing only the peak is a meter that never settles.

use std::time::Duration;

use crate::audio::Levels;
use crate::ui::hold::{Window, frames_in};

const OPEN: char = '[';
const CLOSE: char = ']';
const BRACKETS: usize = 2;
const FILLED: char = '#';
const UNFILLED: char = '-';
const MARKER: char = '|';
const HOLD: Duration = Duration::from_secs(1);
const DECIBELS_PER_DECADE: f32 = 20.0;

/// A bar showing how loud the input is, with the recent peak marked on it.
///
/// One reading a frame: the hold advances with the frames drawn rather than
/// with a clock, so a meter nobody is drawing keeps whatever it last showed.
///
/// ```
/// use motif::audio::Levels;
/// use motif::ui::LevelMeter;
///
/// let mut meter = LevelMeter::new();
///
/// assert_eq!(meter.bar(Levels::SILENT, 8), "[------]");
/// assert_eq!(meter.bar(Levels { peak: 1.0, rms: 1.0 }, 8), "[#####|]");
/// ```
#[derive(Debug)]
pub struct LevelMeter {
    peaks: Window,
}

impl LevelMeter {
    /// The quietest level the bar shows, in decibels below full scale.
    ///
    /// Anything under it draws as nothing. Sixty decibels is the range a
    /// hardware meter of this size is conventionally given: it reaches the
    /// noise floor of a line input without spending cells no signal will
    /// reach.
    pub const FLOOR_DBFS: f32 = -60.0;

    /// How many frames a peak stays up for.
    ///
    /// A second's worth. What the hold is for is a person catching a spike, so
    /// the window is what one has to survive to be seen at all: a marker
    /// decaying over two frames is gone in 66 ms. A peak outlives this by up to
    /// one more window, the window it is still filling when it arrives.
    pub const PEAK_HOLD_FRAMES: usize = frames_in(HOLD);

    /// A meter showing nothing.
    pub fn new() -> Self {
        Self {
            peaks: Window::spanning(Self::PEAK_HOLD_FRAMES),
        }
    }

    /// Take `levels` as this frame's reading, and answer the bar to draw.
    ///
    /// `columns` is the whole width including the brackets, and the bar fills
    /// exactly that; one too narrow to hold them draws nothing rather than
    /// spilling into the cell beside it. Written into a
    /// [`Region`](crate::ui::Region) by whoever placed it, so the widget never
    /// needs to know where on the screen it ended up.
    pub fn bar(&mut self, levels: Levels, columns: usize) -> String {
        let Some(scale) = columns.checked_sub(BRACKETS) else {
            return String::new();
        };

        let filled = lit(levels.rms, scale);
        let marked = lit(self.peaks.holding(levels.peak), scale).checked_sub(1);

        let mut drawn = String::with_capacity(columns);
        drawn.push(OPEN);
        for cell in 0..scale {
            drawn.push(if Some(cell) == marked {
                MARKER
            } else if cell < filled {
                FILLED
            } else {
                UNFILLED
            });
        }
        drawn.push(CLOSE);

        drawn
    }
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self::new()
    }
}

fn lit(amplitude: f32, scale: usize) -> usize {
    let decibels = DECIBELS_PER_DECADE * amplitude.log10();
    let above = (decibels - LevelMeter::FLOOR_DBFS) / -LevelMeter::FLOOR_DBFS;

    (above * scale as f32).round().clamp(0.0, scale as f32) as usize
}
