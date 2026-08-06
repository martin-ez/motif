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
use std::fmt::{self, Write};

use crate::closed_set;
use crate::device::{Button, Control, Encoder};

closed_set! {
    /// Which way an encoder was turned.
    enum Turn;
    /// Every way an encoder turns.
    ///
    /// A closed set for the reason the panel's controls are one: anything
    /// stepping through the gestures a scheme might bind has to reach every
    /// turn, and a turn added to the panel cannot be left out of the array.
    const ALL;
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

    /// The control on the panel this happened to.
    ///
    /// What a legend, a meter or anything else indexing by control asks for, so
    /// that reaching one does not mean matching on the shape of the event and
    /// gaining a case to forget when the panel grows a control.
    pub const fn control(self) -> Control {
        match self {
            Self::Turned { encoder, .. } => Control::Encoder(encoder),
            Self::Pressed { button, .. } => Control::Button(button),
        }
    }
}

/// What a panel names the way a control is reached by.
///
/// A fixed-size value rather than a string, because it is asked for while a
/// frame is being drawn and a legend that allocated would put an allocation per
/// control into every frame. [`CAPACITY`](Self::CAPACITY) is three glyphs,
/// which holds a key, or the pair of keys a terminal turns an encoder with
/// written as `q/w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    glyphs: [char; Hint::CAPACITY],
    filled: usize,
}

impl Hint {
    /// The most glyphs a hint carries.
    pub const CAPACITY: usize = 3;

    /// A hint showing `glyphs`, of which it keeps the first
    /// [`CAPACITY`](Self::CAPACITY).
    ///
    /// Clipped rather than refused, because a hint is drawn into an entry that
    /// is narrow anyway: a backend naming a control with more than this has a
    /// legend that no longer fits, which is a layout to settle rather than a
    /// frame to stop drawing.
    pub fn new(glyphs: impl IntoIterator<Item = char>) -> Self {
        let mut hint = Self {
            glyphs: [' '; Self::CAPACITY],
            filled: 0,
        };

        for glyph in glyphs.into_iter().take(Self::CAPACITY) {
            hint.glyphs[hint.filled] = glyph;
            hint.filled += 1;
        }

        hint
    }

    /// The glyphs, in the order the panel gave them.
    pub fn glyphs(self) -> impl Iterator<Item = char> {
        (0..self.filled).map(move |at| self.glyphs[at])
    }
}

impl fmt::Display for Hint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.glyphs().try_for_each(|glyph| f.write_char(glyph))
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

    /// What to call the way this panel reaches `control`, if that is worth
    /// drawing.
    ///
    /// A terminal answers with the key, because nothing in front of the player
    /// is labelled with it. A panel whose controls are labelled where the
    /// player's hands already are has nothing to add, which is what the default
    /// answers.
    ///
    /// A hint travels as glyphs and is drawn, never matched on, so nothing above
    /// the backend learns what reached it.
    fn hint(&self, _control: Control) -> Option<Hint> {
        None
    }

    /// Whether the panel has ended the run itself.
    ///
    /// A panel with a way out the application neither declares nor can refuse
    /// answers true once it has been taken, and the loop stops without asking.
    /// It exists because a run can outlast the gesture that was meant to end
    /// it, and one that cannot be ended where it was started has to be killed
    /// from somewhere else.
    ///
    /// The default is false: a panel whose every control reaches the
    /// application has no way out the application does not already hold.
    fn interrupted(&self) -> bool {
        false
    }
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
