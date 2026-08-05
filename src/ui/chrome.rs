//! The rows around the application: what it is, and how to leave it.

use crate::ui::{App, ControlEvent, Flow, Legend, Region, columns_of};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));
const QUIT: &str = "shift + stop to quit";
const CHROME_ROWS: usize = 1;

fn write_right(region: &mut Region<'_>, text: &str) {
    let column = region.columns().saturating_sub(columns_of(text));

    region.write(column, 0, text);
}

/// An application with the instrument's name above it and the way out below.
///
/// Two rows the application never sees: they are taken off the region before it
/// is handed the rest, so a page filling every row it was given cannot land on
/// one and the chrome cannot land on a page's. Everything else — the controls,
/// the legend, whether the run goes on — is the application's, passed through.
///
/// The way out is drawn whatever the application answers, because it is the
/// [`Shell`](crate::ui::Shell)'s and not a page's: a screen that ignored stop
/// would otherwise leave a player with no stated way back to their terminal.
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
        let (mut name, below) = region.split_top(CHROME_ROWS);
        let (application, mut quit) = below.split_bottom(CHROME_ROWS);

        write_right(&mut name, NAME);
        write_right(&mut quit, QUIT);

        self.app.draw(application)
    }
}
