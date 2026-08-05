//! What the application means by a control, as against what the player did.
//!
//! An [`Intent`] names a screen to reach and never a control that reaches it,
//! and a [`Navigation`] is what turns the one into the other. The
//! [`Shell`](crate::ui::Shell) resolves an event before any page sees it, so a
//! page never learns which control navigates and a scheme is one value rather
//! than an opinion held by every page.
//!
//! What that value is belongs to whoever composes the application, the way
//! choosing a backend does: nothing here binds a control to anything.

use crate::ui::{ControlEvent, Mode};

/// Something the application should do about where it is.
///
/// The vocabulary a page would navigate in if it wanted to, which is why no
/// variant names a control: a scheme that moves the gesture leaves what the
/// application means untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Show a mode, whatever is showing now.
    Show(Mode),
}

/// A scheme, as the shell consults it: what a control means, if it means
/// anything.
///
/// One method rather than a table of bindings, because the table is the
/// implementation's to choose. A control this resolves is the shell's and never
/// reaches the showing page, so a scheme takes the few controls it is given
/// rather than everything it might want.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{ControlEvent, Intent, Mode, Navigation};
///
/// struct Home;
///
/// impl Navigation for Home {
///     fn intent(&self, event: ControlEvent) -> Option<Intent> {
///         match event {
///             ControlEvent::Pressed { button: Button::FirstScene, .. } => {
///                 Some(Intent::Show(Mode::Looper))
///             }
///             _ => None,
///         }
///     }
/// }
///
/// let scene = ControlEvent::Pressed { button: Button::FirstScene, shifted: false };
/// let play = ControlEvent::Pressed { button: Button::Play, shifted: false };
///
/// assert_eq!(Home.intent(scene), Some(Intent::Show(Mode::Looper)));
/// assert_eq!(Home.intent(play), None);
/// ```
pub trait Navigation {
    /// What `event` means here, or `None` if it means nothing and belongs to
    /// the page.
    fn intent(&self, event: ControlEvent) -> Option<Intent>;
}
