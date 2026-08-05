//! The shape of the machine `motif` is built for, stated once.
//!
//! Screen, audio and cores are frozen in [`DeviceProfile::TARGET`], and the
//! panel — twelve buttons and one encoder — in [`Encoder`] and [`Button`]. The
//! rest of the crate sizes itself from those rather than from whatever host it
//! happens to run on: a terminal 200 columns wide still draws a
//! [`ScreenProfile::columns`]-wide frame.
//!
//! The numbers are allowed to be wrong. They are not allowed to be implicit,
//! scattered, or discovered at runtime — a profile field is a decision a future
//! hardware backend has to meet. A control is a closed set rather than a count,
//! so a `match` stops compiling when the panel gains one.
//!
//! Everything is available in a constant expression, so buffers can be sized by
//! the compiler:
//!
//! ```
//! use motif::device::{DeviceProfile, Encoder};
//!
//! const CELLS: usize = DeviceProfile::TARGET.screen.cells();
//!
//! let frame = [' '; CELLS];
//! assert_eq!(frame.len(), CELLS);
//!
//! let mut parameters = [0.0; Encoder::ALL.len()];
//! parameters[Encoder::Main as usize] = 0.5;
//! ```

use std::time::Duration;

use crate::closed_set::closed_set;

/// The screen the UI draws into, measured in character cells.
///
/// Cells rather than pixels, so that a terminal and a panel are describable by
/// the same two numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenProfile {
    /// Cells across.
    pub columns: usize,
    /// Cells down.
    pub rows: usize,
    /// Frames drawn per second.
    ///
    /// A stated rate rather than as fast as the machine manages, because the
    /// screen shares a machine with analysis: a UI allowed to be greedy on a
    /// laptop takes time from work that has a deadline on the target.
    pub refresh_rate: u32,
}

impl ScreenProfile {
    /// Cells in a full frame.
    pub const fn cells(self) -> usize {
        self.columns.saturating_mul(self.rows)
    }

    /// How long one frame gets at [`refresh_rate`](Self::refresh_rate).
    ///
    /// A screen that never refreshes has no budget rather than a division by
    /// zero: a profile is data, and one that is wrong should size something
    /// useless instead of failing to build.
    pub const fn frame_budget(self) -> Duration {
        if self.refresh_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(NANOSECONDS_PER_SECOND / self.refresh_rate as u64)
    }
}

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

closed_set! {
    /// An encoder on the panel.
    ///
    /// The panel has one, and it is a closed set of one rather than a bare
    /// marker: what the panel carries is a decision, so a second encoder should
    /// be a variant added here rather than a new type threaded through every
    /// consumer. Unlike a [`Button`] it carries no meaning of its own — it
    /// adjusts whatever the page beneath it is showing.
    ///
    /// [`ALL`](Self::ALL) is generated from the declaration, so an encoder
    /// cannot be added to the panel and left out of the array.
    enum Encoder;
    /// Every encoder, left to right.
    ///
    /// An encoder's position here is its discriminant, so `encoder as usize`
    /// indexes an array sized by `ALL.len()`.
    const ALL;
    /// The encoder beside the screen.
    Main,
}

closed_set! {
    /// A button on the panel, named for what it does rather than numbered.
    ///
    /// Naming them is what lets a backend's key mapping be checked: a `match`
    /// over this enum stops compiling when the panel gains a button. The scene
    /// buttons are the exception, named for where they sit, because which scene
    /// a button selects is a fact about the song rather than about the panel.
    ///
    /// Shift is a button the panel has, so it is named here, but a backend folds
    /// it into [`ControlEvent::is_shifted`](crate::ui::ControlEvent) rather than
    /// sending it as a press of its own.
    enum Button;
    /// Every button, in panel order.
    ///
    /// A button's position here is its discriminant, so `button as usize`
    /// indexes an array sized by `ALL.len()`.
    const ALL;
    /// Navigate up.
    Up,
    /// Navigate down.
    Down,
    /// Navigate left.
    Left,
    /// Navigate right.
    Right,
    /// The leftmost scene button.
    FirstScene,
    /// The second scene button from the left.
    SecondScene,
    /// The third scene button from the left.
    ThirdScene,
    /// The rightmost scene button.
    FourthScene,
    /// Start playback.
    Play,
    /// Halt playback.
    Stop,
    /// Arm capture.
    Record,
    /// Change what the next control does.
    Shift,
}

/// A control on the panel, of whichever kind.
///
/// [`Button`] and [`Encoder`] are separate sets because a button is pressed and
/// an encoder is turned, and an event is one or the other. Anything describing
/// the panel as a whole — which controls a page answers, how a backend reaches
/// them — needs them as one set, and this is it.
///
/// [`ALL`](Self::ALL) is derived from the two arrays rather than written out, so
/// a control added to the panel cannot be left out of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// A button, which is pressed.
    Button(Button),
    /// An encoder, which is turned.
    Encoder(Encoder),
}

impl Control {
    /// Every control on the panel: the buttons in panel order, then the
    /// encoders left to right.
    ///
    /// A control's place here is its [`position`](Self::position), so an array
    /// sized by `ALL.len()` holds one entry per control.
    pub const ALL: [Self; Button::ALL.len() + Encoder::ALL.len()] = Self::listed();

    /// Where this control sits in [`ALL`](Self::ALL).
    pub const fn position(self) -> usize {
        match self {
            Self::Button(button) => button as usize,
            Self::Encoder(encoder) => Button::ALL.len() + encoder as usize,
        }
    }

    const fn listed() -> [Self; Button::ALL.len() + Encoder::ALL.len()] {
        let mut listed = [Self::Button(Button::Up); Button::ALL.len() + Encoder::ALL.len()];
        let mut at = 0;

        while at < Button::ALL.len() {
            listed[at] = Self::Button(Button::ALL[at]);
            at += 1;
        }
        while at < listed.len() {
            listed[at] = Self::Encoder(Encoder::ALL[at - Button::ALL.len()]);
            at += 1;
        }

        listed
    }
}

impl From<Button> for Control {
    fn from(button: Button) -> Self {
        Self::Button(button)
    }
}

impl From<Encoder> for Control {
    fn from(encoder: Encoder) -> Self {
        Self::Encoder(encoder)
    }
}

/// The audio device the engine is built against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioProfile {
    /// Frames per second.
    pub sample_rate: u32,
    /// Frames per callback.
    pub block_size: u32,
    /// The longest loop that can be captured, in seconds.
    ///
    /// Maximum loop length is a stated constraint of the device rather than an
    /// accident of how much memory happens to be free, because on hardware it
    /// is one either way.
    pub max_loop_seconds: u32,
}

impl AudioProfile {
    /// Frames in the longest capturable loop.
    ///
    /// Saturates rather than wrapping, so a profile too large for the target's
    /// pointer width sizes a buffer that cannot be allocated instead of one
    /// quietly too small to hold the loop.
    pub const fn max_loop_frames(self) -> usize {
        (self.sample_rate as usize).saturating_mul(self.max_loop_seconds as usize)
    }
}

/// The shape of the machine `motif` runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceProfile {
    /// The screen a frame is drawn into.
    pub screen: ScreenProfile,
    /// The audio device the engine is built against.
    pub audio: AudioProfile,
    /// Cores the whole program has to share.
    ///
    /// One of them is spoken for: the audio callback runs on a core of its own,
    /// and analysis has to fit in what is left.
    pub cores: usize,
}

impl DeviceProfile {
    /// The device `motif` is built for.
    ///
    /// A 5-inch 800×480 IPS panel drawn with a 12×24 cell: 66 columns by 20 rows,
    /// a character of 1.63 × 3.27 mm, and a frame that still fits an 80×24
    /// terminal. That character size is where the instruments this is built after
    /// sit — an OP-1 at 1.59 × 3.18 mm, a Polyend Tracker at 1.52 × 3.05 mm.
    ///
    /// It refreshes 30 times a second, and the audio is what a class-compliant
    /// USB interface offers everywhere: 48 kHz in 256-frame blocks, a 5.33 ms
    /// deadline. The panel is not here; [`Encoder`] and [`Button`] state it.
    pub const TARGET: Self = Self {
        screen: ScreenProfile {
            columns: 66,
            rows: 20,
            refresh_rate: 30,
        },
        audio: AudioProfile {
            sample_rate: 48_000,
            block_size: 256,
            max_loop_seconds: 32,
        },
        cores: 4,
    };
}
