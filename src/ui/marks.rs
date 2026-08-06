//! What a control shows the moment its event reaches the application.
//!
//! A frame is drawn from state and no backend remembers the last one, so a mark
//! painted once would be gone before anyone saw it. It lives here instead: set
//! where the event is handed over, read where the panel is drawn, and aged a
//! frame at a time until it is back at rest.
//!
//! Counted in frames rather than milliseconds. The loop already runs to a frame
//! budget, and a mark fading on a clock of its own would be a second clock to
//! keep in step with it.

use crate::device::{Control, Encoder};
use crate::ui::{ControlEvent, Turn};

/// Which controls have just been handed an event, and how much longer each one
/// shows it.
///
/// A byte per control, so it copies rather than being threaded by reference.
/// Delivery is what sets a mark, not the page: a control the application
/// ignores is marked exactly as one it acts on, which is what makes the path
/// from a key to a page observable by running the thing.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{ControlEvent, Marks};
///
/// let mut marks = Marks::none();
/// marks.fired(ControlEvent::Pressed { button: Button::Play, shifted: false });
///
/// assert!(marks.marked(Button::Play));
/// assert!(!marks.marked(Button::Stop));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    remaining: [u8; Control::ALL.len()],
    turns: [Turn; Encoder::ALL.len()],
}

impl Marks {
    /// How many frames a control stays marked once its event is delivered.
    ///
    /// Three, which is about a tenth of a second at the target's refresh rate:
    /// long enough to catch on a screen someone is watching, short enough that
    /// two presses in quick succession stay two marks.
    pub const FRAMES: u8 = 3;

    /// Every control at rest.
    pub const fn none() -> Self {
        Self {
            remaining: [0; Control::ALL.len()],
            turns: [Turn::Clockwise; Encoder::ALL.len()],
        }
    }

    /// `event` has just been handed to the application: mark the control it
    /// reached.
    ///
    /// A control firing again before it settles starts the count over, so a key
    /// held against a panel that repeats stays marked for as long as the
    /// repeats arrive and settles once they stop. An encoder turned back the
    /// other way inside that window shows the new direction.
    pub fn fired(&mut self, event: ControlEvent) {
        let control = match event {
            ControlEvent::Pressed { button, .. } => Control::Button(button),
            ControlEvent::Turned { encoder, turn, .. } => {
                self.turns[encoder as usize] = turn;
                Control::Encoder(encoder)
            }
        };

        self.remaining[control.position()] = Self::FRAMES;
    }

    /// Whether `control` is still showing its mark.
    pub fn marked(&self, control: impl Into<Control>) -> bool {
        self.remaining[control.into().position()] > 0
    }

    /// Which way `encoder` was turned, while it is still showing the mark.
    ///
    /// `None` once it has settled, so a caller drawing the side that moved has
    /// one answer to ask for rather than a direction and a mark to combine.
    pub fn turn(&self, encoder: Encoder) -> Option<Turn> {
        self.marked(encoder).then(|| self.turns[encoder as usize])
    }

    /// A frame has been drawn: every mark is one frame closer to rest.
    ///
    /// A control already at rest stays there rather than counting round again,
    /// which is what lets the loop age the whole set every frame without asking
    /// what is marked.
    pub fn age(&mut self) {
        for remaining in &mut self.remaining {
            *remaining = remaining.saturating_sub(1);
        }
    }
}

impl Default for Marks {
    fn default() -> Self {
        Self::none()
    }
}
