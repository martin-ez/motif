//! What the application means by a control, as against what the player did.
//!
//! An [`Intent`] names a screen to reach and never a control that reaches it,
//! and a [`Navigation`] is what turns the one into the other. The
//! [`Shell`](crate::ui::Shell) resolves an event before any page sees it, so a
//! page never learns which control navigates and a scheme is one value rather
//! than an opinion held by every page.
//!
//! [`Scheme`] is that value written as a table, and the mechanism takes any
//! table: a composition that wants different gestures passes different rows.
//! [`Scheme::scenes`] is the one the instrument runs today, named here rather
//! than typed out where the application is built, so that a test can assert the
//! bindings and a change to them is a change to one value.

use crate::device::{Button, Control, Encoder};
use crate::ui::{ControlEvent, Legend, Mode, Turn};

const SHIFTS: [bool; 2] = [false, true];

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

/// A scheme written as a table: the gestures that navigate, each beside what it
/// means.
///
/// Rows rather than an entry per control, so a button added to the panel is a
/// row here and nothing else: a scheme states the gestures it binds and never
/// assumes the set they were drawn from. A gesture no row names means nothing,
/// and reaches the showing page unchanged.
///
/// A value, so trying a different scheme is a different table rather than a
/// rewritten `match`.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{ControlEvent, Intent, Mode, Navigation, Scheme};
///
/// let scene = ControlEvent::Pressed { button: Button::FirstScene, shifted: false };
/// let play = ControlEvent::Pressed { button: Button::Play, shifted: false };
///
/// let scheme = Scheme::new([(scene, Intent::Show(Mode::Looper))]);
///
/// assert_eq!(scheme.intent(scene), Some(Intent::Show(Mode::Looper)));
/// assert_eq!(scheme.intent(play), None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    bindings: Vec<(ControlEvent, Intent)>,
}

impl Scheme {
    /// A scheme resolving `bindings` and no gesture they leave out.
    ///
    /// The rows are taken as they are given rather than checked against the
    /// panel, which is what lets a scheme outlive a change to it.
    pub fn new(bindings: impl IntoIterator<Item = (ControlEvent, Intent)>) -> Self {
        Self {
            bindings: bindings.into_iter().collect(),
        }
    }

    /// The scheme the instrument navigates by: a scene button per mode, in the
    /// order the modes sit in.
    ///
    /// A row per [`Mode`] rather than a ring, so what reaches a screen is one
    /// press wherever the player is and the topology is the mode set rather
    /// than an order written into a step. The scene buttons are what is free:
    /// no page answers one, so nothing a scheme takes was doing something else.
    ///
    /// ```
    /// use motif::device::Button;
    /// use motif::ui::{ControlEvent, Intent, Mode, Navigation, Scheme};
    ///
    /// let scene = ControlEvent::Pressed { button: Button::SecondScene, shifted: false };
    ///
    /// assert_eq!(Scheme::scenes().intent(scene), Some(Intent::Show(Mode::Settings)));
    /// ```
    pub fn scenes() -> Self {
        Self::new(Mode::ALL.map(|mode| (pressing(reaching(mode)), Intent::Show(mode))))
    }
}

fn reaching(mode: Mode) -> Button {
    match mode {
        Mode::Looper => Button::FirstScene,
        Mode::Settings => Button::SecondScene,
    }
}

fn pressing(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

impl Navigation for Scheme {
    fn intent(&self, event: ControlEvent) -> Option<Intent> {
        self.bindings
            .iter()
            .find(|(gesture, _)| *gesture == event)
            .map(|&(_, intent)| intent)
    }
}

/// Which controls `navigation` keeps for itself, drawn as a legend.
///
/// Asked of the same value the shell navigates by, gesture by gesture, so the
/// keys the picture lights are the scheme rather than a second copy of it that
/// nothing checks. Changing the scheme changes what the screen says.
///
/// It joins the showing page's legend rather than taking a row of its own: a
/// [`Panel`](crate::ui::Panel) is drawn beside the frame and costs the screen
/// nothing, where a row of its own would spend one of the fifteen. A key drawn
/// heavy then means the same everywhere — this does something here.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{Scheme, navigating};
///
/// let legend = navigating(&Scheme::scenes());
///
/// assert!(legend.answers(Button::FirstScene));
/// assert!(!legend.answers(Button::Play));
/// ```
pub fn navigating(navigation: &dyn Navigation) -> Legend {
    let pressed = Button::ALL
        .into_iter()
        .filter(|&button| resolves(navigation, presses(button)))
        .map(Control::Button);
    let turned = Encoder::ALL
        .into_iter()
        .filter(|&encoder| resolves(navigation, turns(encoder)))
        .map(Control::Encoder);

    pressed
        .chain(turned)
        .fold(Legend::blank(), |legend, control| legend.answering(control))
}

fn resolves(navigation: &dyn Navigation, gestures: impl IntoIterator<Item = ControlEvent>) -> bool {
    gestures
        .into_iter()
        .any(|event| navigation.intent(event).is_some())
}

fn presses(button: Button) -> impl Iterator<Item = ControlEvent> {
    SHIFTS
        .into_iter()
        .map(move |shifted| ControlEvent::Pressed { button, shifted })
}

fn turns(encoder: Encoder) -> impl Iterator<Item = ControlEvent> {
    Turn::ALL.into_iter().flat_map(move |turn| {
        SHIFTS.map(move |shifted| ControlEvent::Turned {
            encoder,
            turn,
            shifted,
        })
    })
}
