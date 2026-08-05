//! The application around the pages: which one is showing, and what never
//! reaches it.

use crate::device::Button;
use crate::ui::{App, ControlEvent, Flow, Legend, Mode, Page, Region};

/// The pages the instrument has, and the one it is showing.
///
/// A page per [`Mode`], held in an array the mode indexes, so `Mode::ALL` sizes
/// it and a mode with no page cannot be expressed. Which pages those are is a
/// composition question, the way choosing a backend is.
///
/// Showing a mode is a call, not a gesture: the shell is what does not change
/// when a navigation scheme does, so it is built without one.
///
/// It keeps shift + stop and forwards the rest to the showing page, declaring
/// both halves so the way out is drawn live on a page that ignores stop.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{App, Cell, ControlEvent, Frame, Legend, Mode, Page, Region, Shell};
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
///     fn draw(&mut self, mut region: Region<'_>) {
///         region.set(0, 0, Cell::new('m'));
///     }
/// }
///
/// let mut shell = Shell::new([Box::new(Blank)]);
/// shell.show(Mode::Looper);
///
/// let mut frame = Frame::blank();
/// shell.draw(frame.region());
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

    fn draw(&mut self, region: Region<'_>) -> Flow {
        self.page_mut().draw(region);

        Flow::Continue
    }
}
