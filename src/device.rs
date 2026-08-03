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
//! parameters[Encoder::Third as usize] = 0.5;
//! ```

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
}

impl ScreenProfile {
    /// Cells in a full frame.
    pub const fn cells(self) -> usize {
        self.columns.saturating_mul(self.rows)
    }
}

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
    /// An encoder on the panel, named for where it sits.
    ///
    /// The name is the position and nothing else: an encoder adjusts whatever
    /// the page beneath it is showing, so unlike a [`Button`] it carries no
    /// meaning of its own. It is a closed set rather than a count because an
    /// encoder the panel does not have should not be expressible.
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
    /// The leftmost encoder.
    First,
    /// The second encoder from the left.
    Second,
    /// The third encoder from the left.
    Third,
    /// The rightmost encoder.
    Fourth,
}

panel_control! {
    /// A button on the panel, named for what it is rather than numbered.
    ///
    /// Naming them is what lets a backend's key mapping be checked: a `match`
    /// over this enum stops compiling when the panel gains a button, so the
    /// terminal's table of keys cannot silently fall behind the device. As with
    /// [`Encoder`], the set is declared once and [`ALL`](Self::ALL) is
    /// generated from it, so the two cannot drift apart.
    ///
    /// Shift is absent deliberately. It is a modifier — it changes what another
    /// control means rather than meaning anything alone — so a backend resolves
    /// it and stamps it onto the event. A `Shift` variant here would put the
    /// held state back into every consumer, which is key handling with a new
    /// name.
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
    /// Start playback.
    Play,
    /// Halt playback.
    Stop,
    /// Arm capture.
    Record,
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
    /// The screen is a 320×240 panel drawn with an 8×16 cell, which is 40
    /// columns by 15 rows — small enough that a default 80×24 terminal can
    /// always show a whole frame. The audio device is the configuration a
    /// class-compliant USB interface offers everywhere: 48 kHz in blocks of
    /// 256 frames, which is 5.33 ms of deadline per callback. Four cores is a
    /// quad-core ARM board of the kind this would be built on.
    ///
    /// The panel is not here. [`Encoder`] and [`Button`] are closed sets, so
    /// they state it themselves and a field repeating their length would be a
    /// second place to change.
    pub const TARGET: Self = Self {
        screen: ScreenProfile {
            columns: 40,
            rows: 15,
        },
        audio: AudioProfile {
            sample_rate: 48_000,
            block_size: 256,
            max_loop_seconds: 32,
        },
        cores: 4,
    };
}
