//! What the player is doing with the loop, and what that makes of a block.
//!
//! [`Transport::record`], [`Transport::play`] and [`Transport::stop`] are the
//! only way to change state: each takes the current state and returns the next.
//! Nothing here accepts a state to move to, so an illegal transition is not
//! rejected at runtime — there is no way to ask for one. Each action is defined
//! for all five states, so none of them returns an error.
//!
//! The transport says what to do with a block, never how many frames a take
//! captured: a record and a stop arriving in the same drain leave
//! [`Transport::Stopped`] over an empty buffer, so a consumer reads the buffer
//! for length and handles a loop of no frames.
//!
//! Every transition is a `const fn` over a `Copy` value, so applying one has
//! nothing to allocate with.

/// What the looper is doing with the loop.
///
/// ```
/// use motif::looper::Transport;
///
/// let take = Transport::Idle.record();
/// assert!(take.captures_input());
///
/// let layer = take.record();
/// assert!(layer.captures_input() && layer.plays_loop());
///
/// assert_eq!(layer.record(), Transport::Playing);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    /// Nothing has been recorded, so there is nothing to play.
    #[default]
    Idle,
    /// The first take is being captured, with no loop yet to play underneath
    /// it.
    Recording,
    /// The loop is playing and the input is not being captured.
    Playing,
    /// The loop is playing and the input is being layered onto it.
    Overdubbing,
    /// A take has been made and nothing is playing, unlike [`Transport::Idle`]
    /// which has none to resume.
    Stopped,
}

impl Transport {
    /// Capture onto the loop: open the first take, layer over the loop that
    /// exists, or drop back out of the layer that is open.
    #[must_use]
    pub const fn record(self) -> Self {
        match self {
            Self::Idle => Self::Recording,
            Self::Recording | Self::Playing | Self::Stopped => Self::Overdubbing,
            Self::Overdubbing => Self::Playing,
        }
    }

    /// Play the loop without capturing, closing a take or a layer that was
    /// open. [`Transport::Idle`] has nothing to play and is left alone.
    #[must_use]
    pub const fn play(self) -> Self {
        match self {
            Self::Idle => Self::Idle,
            Self::Recording | Self::Playing | Self::Overdubbing | Self::Stopped => Self::Playing,
        }
    }

    /// Halt, keeping what has been recorded.
    #[must_use]
    pub const fn stop(self) -> Self {
        match self {
            Self::Idle => Self::Idle,
            Self::Recording | Self::Playing | Self::Overdubbing | Self::Stopped => Self::Stopped,
        }
    }

    /// Whether the input arriving in this block belongs in the loop.
    pub const fn captures_input(self) -> bool {
        match self {
            Self::Recording | Self::Overdubbing => true,
            Self::Idle | Self::Playing | Self::Stopped => false,
        }
    }

    /// Whether the loop should be heard in this block.
    pub const fn plays_loop(self) -> bool {
        match self {
            Self::Playing | Self::Overdubbing => true,
            Self::Idle | Self::Recording | Self::Stopped => false,
        }
    }
}
