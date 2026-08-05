//! One screen of the application, as the shell showing it sees it.

use crate::ui::{ControlEvent, Frame, Legend};

/// A screen a [`Shell`](crate::ui::Shell) can show.
///
/// The same three members an [`App`](crate::ui::App) has, less the
/// [`Flow`](crate::ui::Flow): a page cannot end the run, having nothing to
/// return that would say so. Quitting belongs to the shell.
///
/// A page is handed the whole frame and keeps all of it: what it declares is
/// drawn beside the screen, never over it.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{Cell, ControlEvent, Frame, Legend, Page};
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
/// ```
pub trait Page {
    /// Take one thing the player did.
    ///
    /// Called once per event, for every event the shell did not keep.
    fn control(&mut self, event: ControlEvent);

    /// Which controls this page answers, and what each one does here.
    ///
    /// Required rather than defaulted, for the reason [`App::legend`] is: a
    /// page that says nothing has decided to say nothing, instead of having
    /// forgotten to.
    ///
    /// [`App::legend`]: crate::ui::App::legend
    fn legend(&self) -> Legend;

    /// Put the page's state on `frame`.
    ///
    /// The frame arrives blank.
    fn draw(&mut self, frame: &mut Frame);
}
