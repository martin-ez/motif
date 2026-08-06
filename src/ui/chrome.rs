//! The row above the application: what instrument it is.

use crate::ui::{App, ControlEvent, Flow, Legend, Region, columns_of};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));
const CHROME_ROWS: usize = 1;

fn write_right(region: &mut Region<'_>, text: &str) {
    let column = region.columns().saturating_sub(columns_of(text));

    region.write(column, 0, text);
}

/// An application with the instrument's name above it.
///
/// One row the application never sees: it is taken off the region before the
/// application is handed the rest, so a page filling every row it was given
/// cannot land on it and the chrome cannot land on a page's. Everything else —
/// the controls, the legend, whether the run goes on — is the application's,
/// passed through.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{App, Cell, Chrome, ControlEvent, Flow, Frame, Legend, Region};
///
/// struct Blank;
///
/// impl App for Blank {
///     fn control(&mut self, _event: ControlEvent) -> Flow {
///         Flow::Continue
///     }
///
///     fn legend(&self) -> Legend {
///         Legend::blank().answering(Button::Play)
///     }
///
///     fn draw(&mut self, mut region: Region<'_>) -> Flow {
///         region.set(0, 0, Cell::new('m'));
///
///         Flow::Continue
///     }
/// }
///
/// let mut chrome = Chrome::around(Blank);
///
/// let mut frame = Frame::blank();
/// chrome.draw(frame.region());
///
/// assert_eq!(frame.get(0, 1), Some(Cell::new('m')));
/// ```
pub struct Chrome<A: App> {
    app: A,
}

impl<A: App> Chrome<A> {
    /// Put the chrome around `app`.
    pub const fn around(app: A) -> Self {
        Self { app }
    }
}

impl<A: App> App for Chrome<A> {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.app.control(event)
    }

    fn legend(&self) -> Legend {
        self.app.legend()
    }

    fn draw(&mut self, region: Region<'_>) -> Flow {
        let (mut name, application) = region.split_top(CHROME_ROWS);

        write_right(&mut name, NAME);

        self.app.draw(application)
    }
}
