//! The application around the pages: which one is showing, and what never
//! reaches it.

use crate::device::Button;
use crate::ui::{App, ControlEvent, Flow, Frame, Legend, Mode, Page};

/// The pages the instrument has, and the one it is showing.
///
/// A page per [`Mode`], held in an array the mode indexes: order is part of the
/// mode set rather than an accident of who built the array, so `Mode::ALL`
/// sizes it and a mode with no page behind it cannot be expressed. Which pages
/// those are is a composition question and belongs to whoever builds the shell,
/// the way choosing a backend does — the shell knows it has a page per mode,
/// not which pages they are.
///
/// Showing a mode is a call rather than a gesture. Nothing on the panel reaches
/// it yet, which is deliberate: the shell is the part that does not change when
/// a navigation scheme does, so it is built and tested without one.
///
/// The shell keeps shift + stop for itself and forwards every other control to
/// the showing page. Ending the run is the one decision that cannot belong to a
/// screen, because a screen only knows about itself.
///
/// Both halves of that gesture are added to the showing page's legend, so the
/// way out is drawn as a live key on every page rather than only on the ones
/// that happen to answer stop themselves. A player who cannot see the exit has
/// a terminal they can only leave by killing the process.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{App, Cell, ControlEvent, Frame, Legend, Mode, Page, Shell};
///
/// struct Blank;
///
/// impl Page for Blank {
///     fn control(&mut self, _event: ControlEvent) {}
///
///     fn legend(&self) -> Legend {
///         Legend::blank().answering(Button::Play)
///     }
///
///     fn draw(&mut self, frame: &mut Frame) {
///         frame.set(0, 0, Cell::new('m'));
///     }
/// }
///
/// let mut shell = Shell::new([Box::new(Blank)]);
/// shell.show(Mode::Looper);
///
/// let mut frame = Frame::blank();
/// shell.draw(&mut frame);
///
/// assert_eq!(shell.showing(), Mode::Looper);
/// assert_eq!(frame.get(0, 0), Some(Cell::new('m')));
/// ```
pub struct Shell {
    pages: [Box<dyn Page>; Mode::ALL.len()],
    showing: Mode,
}

impl Shell {
    /// A shell over `pages`, one per [`Mode`] and in that order, showing the
    /// first of them.
    ///
    /// The first mode is where the instrument opens, that being what the order
    /// of the set is for.
    pub fn new(pages: [Box<dyn Page>; Mode::ALL.len()]) -> Self {
        Self {
            pages,
            showing: Mode::ALL[0],
        }
    }

    /// Show `mode`.
    ///
    /// The page that was showing is kept rather than dropped, so coming back to
    /// it finds it as it was left.
    pub fn show(&mut self, mode: Mode) {
        self.showing = mode;
    }

    /// Which mode is showing.
    pub const fn showing(&self) -> Mode {
        self.showing
    }

    fn page(&self) -> &dyn Page {
        self.pages[self.showing as usize].as_ref()
    }

    fn page_mut(&mut self) -> &mut dyn Page {
        self.pages[self.showing as usize].as_mut()
    }
}

impl App for Shell {
    fn control(&mut self, event: ControlEvent) -> Flow {
        if matches!(
            event,
            ControlEvent::Pressed {
                button: Button::Stop,
                shifted: true,
            }
        ) {
            return Flow::Exit;
        }

        self.page_mut().control(event);

        Flow::Continue
    }

    fn legend(&self) -> Legend {
        self.page()
            .legend()
            .answering(Button::Shift)
            .answering(Button::Stop)
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        self.page_mut().draw(frame);

        Flow::Continue
    }
}
