//! One screen of the application, as the shell showing it sees it.

use crate::ui::{ControlEvent, Frame, Legend};

/// A screen a [`Shell`](crate::ui::Shell) can show.
///
/// The same three members an [`App`](crate::ui::App) has, less the
/// [`Flow`](crate::ui::Flow): a page cannot end the run, because there is
/// nothing it could return to say so. Quitting belongs to the shell, which is
/// the part that knows whether there is anywhere else to go — a screen only
/// knows about itself.
///
/// A page is handed the whole frame, of which the bottom [`Legend::ROWS`] rows
/// are drawn over afterwards with what it declared. Nothing here names a key, a
/// terminal or an escape sequence, so the same page draws on a hardware panel
/// once there is one.
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
