//! What the player did, named after the panel rather than after a keyboard.
//!
//! An application reads [`ControlEvent`]s: an encoder was turned, a button was
//! pressed. No key, key code or keyboard is nameable from here, and none may
//! become nameable — a terminal is one way to reach the panel, and the panel is
//! the thing the application is written against. A backend maps whatever it has
//! onto these events, so a firmware backend reading GPIO replaces the terminal
//! without anything above changing.
//!
//! ```
//! use motif::device::Button;
//! use motif::ui::{ControlEvent, Controls, ScriptedControls};
//!
//! let play = ControlEvent::Pressed { button: Button::Play, shifted: false };
//! let mut controls = ScriptedControls::new([play]);
//!
//! assert_eq!(controls.poll(), Some(play));
//! assert_eq!(controls.poll(), None);
//! ```

use std::collections::VecDeque;

use crate::device::{Button, Encoder};

/// Which way an encoder was turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Turned up, away from the player's left.
    Clockwise,
    /// Turned down, the other way.
    Anticlockwise,
}

/// Something the player did to a control on the panel.
///
/// An encoder reports one event per detent rather than a position, because a
/// detent is what the hardware emits and what a page needs to know: the value
/// being adjusted lives in the page, not in the knob.
///
/// Shift is a field rather than a [`Button`], because it means nothing on its
/// own — it changes what another control does. Resolving it is the backend's
/// job, so an application matches on the control it was given instead of
/// tracking which keys are being held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEvent {
    /// An encoder was turned one detent.
    Turned {
        /// The encoder that moved.
        encoder: Encoder,
        /// Which way it went.
        turn: Turn,
        /// Whether shift was held.
        shifted: bool,
    },
    /// A button was pressed.
    Pressed {
        /// The button that went down.
        button: Button,
        /// Whether shift was held.
        shifted: bool,
    },
}

impl ControlEvent {
    /// Whether shift was held, whichever control this is.
    pub const fn is_shifted(self) -> bool {
        match self {
            Self::Turned { shifted, .. } | Self::Pressed { shifted, .. } => shifted,
        }
    }
}

/// A panel the application takes control events from.
///
/// [`poll`](Self::poll) returns what has already happened and nothing more: it
/// never waits for the player. The frame budget is spent drawing, and a poll
/// that blocked until a control moved would spend it waiting instead.
pub trait Controls {
    /// The next event, or `None` when nothing is waiting.
    fn poll(&mut self) -> Option<ControlEvent>;
}

/// A panel with no hardware behind it, handing back events given in advance.
///
/// It exists so that an application can be driven where no panel is present,
/// and so that a test can state what the player did as a sequence.
#[derive(Debug, Default)]
pub struct ScriptedControls {
    queued: VecDeque<ControlEvent>,
}

impl ScriptedControls {
    /// A panel that will hand back `events`, in order.
    pub fn new(events: impl IntoIterator<Item = ControlEvent>) -> Self {
        Self {
            queued: events.into_iter().collect(),
        }
    }

    /// Add `event` behind whatever is still waiting.
    pub fn push(&mut self, event: ControlEvent) {
        self.queued.push_back(event);
    }
}

impl Controls for ScriptedControls {
    fn poll(&mut self) -> Option<ControlEvent> {
        self.queued.pop_front()
    }
}
