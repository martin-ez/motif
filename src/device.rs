//! The shape of the machine `motif` is built for, stated once.
//!
//! Screen, controls, audio and cores are frozen in [`DeviceProfile::TARGET`],
//! and the rest of the crate sizes itself from that rather than from whatever
//! the host it happens to be running on reports. A terminal that is 200 columns
//! wide still draws a [`ScreenProfile::columns`]-wide frame, because the screen
//! being aimed at is not the terminal.
//!
//! The numbers are allowed to be wrong. They are not allowed to be implicit,
//! scattered, or discovered at runtime: a profile field is a decision that a
//! future hardware backend has to meet, so changing one is a change to the
//! product rather than to a default.
//!
//! Every field is available in a constant expression, so buffers can be sized
//! by the compiler:
//!
//! ```
//! use motif::device::DeviceProfile;
//!
//! const CELLS: usize = DeviceProfile::TARGET.screen.cells();
//!
//! let frame = [' '; CELLS];
//! assert_eq!(frame.len(), CELLS);
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
        self.columns * self.rows
    }
}

/// The physical controls, counted by kind.
///
/// Counting them by kind rather than in total is what lets input be named after
/// a control — encoder two, button five — instead of after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlProfile {
    /// Rotary encoders, which turn in either direction without a limit and
    /// press.
    pub encoders: usize,
    /// Buttons, which are pressed or not.
    pub buttons: usize,
}

impl ControlProfile {
    /// Controls of every kind together.
    pub const fn total(self) -> usize {
        self.encoders + self.buttons
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
    pub const fn max_loop_frames(self) -> usize {
        self.sample_rate as usize * self.max_loop_seconds as usize
    }
}

/// The shape of the machine `motif` runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceProfile {
    /// The screen a frame is drawn into.
    pub screen: ScreenProfile,
    /// The controls a player reaches for.
    pub controls: ControlProfile,
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
    pub const TARGET: Self = Self {
        screen: ScreenProfile {
            columns: 40,
            rows: 15,
        },
        controls: ControlProfile {
            encoders: 4,
            buttons: 8,
        },
        audio: AudioProfile {
            sample_rate: 48_000,
            block_size: 256,
            max_loop_seconds: 32,
        },
        cores: 4,
    };
}
