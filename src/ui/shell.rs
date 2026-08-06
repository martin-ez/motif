//! The application around the pages: which one is showing, and what never
//! reaches it.

use crate::ui::{
    App, ControlEvent, Flow, Intent, Legend, Mode, Navigation, Page, Region, navigating,
};

/// The pages the instrument has, and the one it is showing.
///
/// A page per [`Mode`], held in an array the mode indexes, so `Mode::ALL` sizes
/// it and a mode with no page cannot be expressed. Which pages those are is a
/// composition question, the way choosing a backend is.
///
/// A control a [`Navigation`] resolves into an [`Intent`] is applied here and
/// not forwarded, so a page never sees what navigates.
///
/// The legend is the page's and [`navigating`]'s at once, so every live key is
/// drawn live. Nothing is kept back: a run ends at the panel it is read from.
///
/// ```
/// use motif::device::Button;
/// use motif::ui::{App, Cell, ControlEvent, Frame, Intent, Legend, Mode, Page, Region, Shell};
///
/// struct Marked(char);
///
/// impl Page for Marked {
///     fn control(&mut self, _event: ControlEvent) {}
///
///     fn legend(&self) -> Legend {
///         Legend::blank().answering(Button::Play)
///     }
///
///     fn draw(&mut self, mut region: Region<'_>) {
///         region.set(0, 0, Cell::new(self.0));
///     }
/// }
///
/// let mut shell = Shell::new([Box::new(Marked('m')), Box::new(Marked('s'))]);
/// shell.apply(Intent::Show(Mode::Settings));
///
/// let mut frame = Frame::blank();
/// shell.draw(frame.region());
///
/// assert_eq!(shell.showing(), Mode::Settings);
/// assert_eq!(frame.get(0, 0), Some(Cell::new('s')));
/// ```
pub struct Shell {
    pages: [Box<dyn Page>; Mode::ALL.len()],
    showing: Mode,
    navigation: Option<Box<dyn Navigation>>,
}

impl Shell {
    /// A shell over `pages`, one per [`Mode`] and in that order, showing the
    /// first of them and navigated by nothing.
    ///
    /// The first mode is where the instrument opens, that being what the order
    /// of the set is for. With no scheme every control reaches the showing
    /// page, which is a shell that has not been given one rather than one that
    /// refuses to navigate.
    pub fn new(pages: [Box<dyn Page>; Mode::ALL.len()]) -> Self {
        Self {
            pages,
            showing: Mode::ALL[0],
            navigation: None,
        }
    }

    /// The same shell, resolving controls through `navigation` first.
    pub fn navigated_by(
        pages: [Box<dyn Page>; Mode::ALL.len()],
        navigation: impl Navigation + 'static,
    ) -> Self {
        Self {
            navigation: Some(Box::new(navigation)),
            ..Self::new(pages)
        }
    }

    /// Do what `intent` asks.
    ///
    /// The one way what is showing changes, whether the intent came from a
    /// scheme or from a caller with no panel in front of it. The page that was
    /// showing is kept rather than dropped, so coming back to it finds it as it
    /// was left.
    pub fn apply(&mut self, intent: Intent) {
        match intent {
            Intent::Show(mode) => self.showing = mode,
        }
    }

    /// Which mode is showing.
    pub const fn showing(&self) -> Mode {
        self.showing
    }

    fn intent(&self, event: ControlEvent) -> Option<Intent> {
        self.navigation.as_ref()?.intent(event)
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
        if let Some(intent) = self.intent(event) {
            self.apply(intent);

            return Flow::Continue;
        }

        self.page_mut().control(event);

        Flow::Continue
    }

    fn legend(&self) -> Legend {
        let page = self.page().legend();

        match self.navigation.as_deref() {
            Some(navigation) => page.also_answering(navigating(navigation)),
            None => page,
        }
    }

    fn draw(&mut self, region: Region<'_>) -> Flow {
        self.page_mut().draw(region);

        Flow::Continue
    }
}
