//! The shape of the machine `motif` is built for, stated once.
//!
//! Screen, audio and cores are frozen in [`DeviceProfile::TARGET`], and the
//! panel is frozen in [`Encoder`] and [`Button`]. The rest of the crate sizes
//! itself from those rather than from whatever the host it happens to be
//! running on reports. A terminal that is 200 columns wide still draws a
//! [`ScreenProfile::columns`]-wide frame, because the screen being aimed at is
//! not the terminal.
//!
//! The numbers are allowed to be wrong. They are not allowed to be implicit,
//! scattered, or discovered at runtime: a profile field is a decision that a
//! future hardware backend has to meet, so changing one is a change to the
//! product rather than to a default.
//!
//! A control is a closed set rather than a count, so a backend that maps input
//! onto the panel can be checked by the compiler: a `match` stops compiling
//! when the panel gains a control, and a control the panel lacks cannot be
//! named at all.
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

macro_rules! panel_control {
    (
        $(#[$control_doc:meta])*
        enum $control:ident;
        $(#[$all_doc:meta])*
        const ALL;
        $($(#[$variant_doc:meta])* $variant:ident,)+
    ) => {
        $(#[$control_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $control {
            $($(#[$variant_doc])* $variant,)+
        }

        impl $control {
            $(#[$all_doc])*
            pub const ALL: [Self; [$(stringify!($variant)),+].len()] = [$(Self::$variant),+];
        }
    };
}

panel_control! {
    /// An encoder on the panel.
    ///
    /// The panel has one, and it is a closed set of one rather than a bare
    /// marker: what the panel carries is a decision, so a second encoder should
    /// be a variant added here rather than a new type threaded through every
    /// consumer. Unlike a [`Button`] it carries no meaning of its own — it
    /// adjusts whatever the page beneath it is showing.
    ///
    /// The set is declared once, and [`ALL`](Self::ALL) is generated from that
    /// declaration. An encoder cannot be added to the panel and left out of the
    /// array, which would compile and then index past the end of anything sized
    /// by `ALL.len()`.
    enum Encoder;
    /// Every encoder, left to right.
    ///
    /// An encoder's position here is its discriminant, so `encoder as usize`
    /// indexes an array sized by `ALL.len()`.
    const ALL;
    /// The encoder beside the screen.
    Main,
}

panel_control! {
    /// A button on the panel, named for what it does rather than numbered.
    ///
    /// Naming them is what lets a backend's key mapping be checked: a `match`
    /// over this enum stops compiling when the panel gains a button, so the
    /// terminal's table of keys cannot silently fall behind the device. As with
    /// [`Encoder`], the set is declared once and [`ALL`](Self::ALL) is
    /// generated from it, so the two cannot drift apart.
    ///
    /// The scene buttons are the exception, and are named for where they sit:
    /// which scene a button selects is a fact about the song rather than about
    /// the panel, so the fourth button is the fourth button whatever is loaded
    /// under it.
    ///
    /// Shift is not a button. It is a modifier — it changes what another
    /// control means rather than meaning anything alone — so a backend resolves
    /// it and stamps it onto the event. A `Shift` variant here would put the
    /// held state back into every consumer, which is key handling with a new
    /// name. It is still a key under the player's hand, so it is drawable as
    /// [`Control::Shift`].
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
    /// The shift key, which is held.
    ///
    /// It is here and not in [`Button`] because it never arrives as an event:
    /// it changes what another control means, and a backend resolves it onto
    /// that control's event. It is still a key on the panel that a player has
    /// to find, so anything drawing the panel has to be able to name it.
    Shift,
}

impl Control {
    /// Every control on the panel: the buttons in panel order, then the
    /// encoders left to right, then shift.
    ///
    /// A control's place here is its [`position`](Self::position), so an array
    /// sized by `ALL.len()` holds one entry per control.
    pub const ALL: [Self; Button::ALL.len() + Encoder::ALL.len() + 1] = Self::listed();

    /// Where this control sits in [`ALL`](Self::ALL).
    pub const fn position(self) -> usize {
        match self {
            Self::Button(button) => button as usize,
            Self::Encoder(encoder) => Button::ALL.len() + encoder as usize,
            Self::Shift => Button::ALL.len() + Encoder::ALL.len(),
        }
    }

    const fn listed() -> [Self; Button::ALL.len() + Encoder::ALL.len() + 1] {
        let mut listed = [Self::Shift; Button::ALL.len() + Encoder::ALL.len() + 1];
        let mut at = 0;

        while at < Button::ALL.len() {
            listed[at] = Self::Button(Button::ALL[at]);
            at += 1;
        }
        while at < Button::ALL.len() + Encoder::ALL.len() {
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
    /// The screen is a 5-inch 800×480 IPS panel drawn with a 12×24 cell, which
    /// is 66 columns by 20 rows. The cell is what fixes the two numbers: at that
    /// size and resolution it puts a character at 1.63 × 3.27 mm, which is where
    /// the instruments this is built after already sit — a Teenage Engineering
    /// OP-1 is 1.59 × 3.18 mm and a Polyend Tracker 1.52 × 3.05 mm, both from
    /// panels near 130 PPI rather than dense ones. A whole frame and the border
    /// a terminal draws around it still fit inside a default 80×24 terminal.
    ///
    /// It refreshes 30 times a second, which is a frame every 33 ms: enough for
    /// a meter to look continuous, and slow enough to leave the cores it shares
    /// with analysis room to work. The audio device is the configuration a
    /// class-compliant USB interface offers everywhere: 48 kHz in blocks of
    /// 256 frames, which is 5.33 ms of deadline per callback. Four cores is a
    /// quad-core ARM board of the kind this would be built on.
    ///
    /// The panel is not here. [`Encoder`] and [`Button`] are closed sets, so
    /// they state it themselves and a field repeating their length would be a
    /// second place to change.
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
