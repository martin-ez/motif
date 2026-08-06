//! One screen of the application, as the shell showing it sees it.

use crate::ui::{ControlEvent, Legend, Region};

/// A screen a [`Shell`](crate::ui::Shell) can show.
///
/// The same three members an [`App`](crate::ui::App) has, less the
/// [`Flow`](crate::ui::Flow): a page cannot end the run, having nothing to
/// return that would say so.
///
/// A page is handed a region and keeps every cell of it: the chrome around it
/// took its rows before the page was called, so nothing is drawn over a page
/// afterwards and a page cannot address a row it was not given.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{Cell, ControlEvent, Legend, Page, Region};
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

    /// Put the page's state on `region`.
    ///
    /// The region arrives blank, and how tall it is depends on what the chrome
    /// above the page took, so a page that fills its rows reads
    /// [`Region::rows`] rather than the screen's height.
    fn draw(&mut self, region: Region<'_>);
}
