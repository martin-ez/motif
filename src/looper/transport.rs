//! What the player is doing with the loop, and what that makes of a block.
//!
//! [`Transport`] is the whole of that state: five states, three actions, and
//! nothing else deciding whether a block is captured or played. The actions are
//! the only way to move between the states — nothing here takes a state as an
//! argument — so a transition the table does not have cannot be written down,
//! let alone rejected at runtime. Every action is defined from every state, so
//! none of them fails, and there is no error to hand a thread that could not do
//! anything with one.
//!
//! The rule the table follows is that record captures onto the loop: it opens
//! the first take, and once there is a loop it opens a layer over it. Dropping
//! back out of that layer is the one exception, and it is what makes record its
//! own inverse over a loop — the player stops layering without stopping the
//! loop. Play plays without capturing, closing a take or a layer that was open,
//! and stop halts.
//!
//! Halted after a take is [`Transport::Stopped`] rather than
//! [`Transport::Idle`], because the two answer both other actions differently:
//! one resumes a take that was made and the other has none to resume. The
//! alternative is one state and a flag beside it saying which of the two it
//! really is, which is a state machine whose state is not all in its state.
//!
//! What the transport does not know is how many frames that take captured. A
//! record and a stop that arrive in the same drain are applied to the same
//! block, so [`Transport::Stopped`] over an empty buffer is reachable and
//! [`Transport::Playing`] after it plays nothing. The states say what to do
//! with a block and the buffer says what there is to do it with, so a consumer
//! reads both: one that plays a loop handles a loop of no frames, and a state
//! here never stands in for a length.
//!
//! Transitions land on block boundaries because of where they are applied,
//! not because anything here waits for one: the callback drains the commands
//! that arrived before its block, and the state it is left holding governs that
//! whole block. An action held here until the next boundary would be the
//! command queue with a second name.
//!
//! Every transition is a `const fn` over a `Copy` value. Applying one cannot
//! allocate, lock or block — not because the callback is careful, but because a
//! function the compiler can evaluate has nothing to allocate with.

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
    /// Nothing has been recorded, so there is nothing to play. Where a looper
    /// begins.
    #[default]
    Idle,
    /// The first take is being captured. Nothing plays underneath it, because
    /// the take is what the loop is about to be.
    Recording,
    /// The loop is playing and the input is not being captured.
    Playing,
    /// The loop is playing and the input is being layered onto it.
    Overdubbing,
    /// A take has been made and nothing is playing. How much of it reached the
    /// buffer is the buffer's to say.
    Stopped,
}

impl Transport {
    /// Capture onto the loop: open the first take, open a layer over the loop
    /// that exists, or drop back out of the layer that is open.
    #[must_use = "a transition is the next state, and applying it is keeping it"]
    pub const fn record(self) -> Self {
        match self {
            Self::Idle => Self::Recording,
            Self::Recording | Self::Playing | Self::Stopped => Self::Overdubbing,
            Self::Overdubbing => Self::Playing,
        }
    }

    /// Play the loop without capturing, closing a take or a layer that was
    /// open.
    ///
    /// [`Transport::Idle`] is the one state this leaves alone: nothing has been
    /// recorded, so there is nothing to play.
    #[must_use = "a transition is the next state, and applying it is keeping it"]
    pub const fn play(self) -> Self {
        match self {
            Self::Idle => Self::Idle,
            Self::Recording | Self::Playing | Self::Overdubbing | Self::Stopped => Self::Playing,
        }
    }

    /// Halt, keeping what has been recorded.
    #[must_use = "a transition is the next state, and applying it is keeping it"]
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
