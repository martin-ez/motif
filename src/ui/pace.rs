//! Measuring how much of a frame budget the event loop spent, and reading it
//! while the loop is still running.
//!
//! A fraction rather than a duration, for the reason
//! [`Headroom`](crate::audio::Headroom) is one: 6 ms means comfortable on a
//! laptop and hopeless on the target, and the fraction is the number that
//! transfers between them.
//!
//! Both ends sit on one thread — the loop calls the application, so there is
//! nothing here to cross — and the shared cell says so where the crate's other
//! meters use an atomic word. Splitting a reading across two atomics to keep
//! that shape would put back exactly the tearing their packing exists to
//! prevent.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use crate::device::DeviceProfile;
use crate::ui::hold::{FRAMES_IN_A_SECOND, Window};

/// How much of the time a frame was allowed to take the loop used.
///
/// The fractions are of one frame budget, where 1.0 is a frame that used
/// exactly its deadline and anything above it is one that overran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// The most recent frame's work as a fraction of the budget.
    pub load: f32,
    /// The largest [`load`](Self::load) over the recent window.
    ///
    /// This is the one that says whether the budget is safe: a mean would hide
    /// the single late frame, which is the whole of what a player sees.
    pub peak: f32,
    /// How many frames have overrun the budget since the run began.
    pub overruns: u64,
}

impl Pace {
    /// A loop that has drawn nothing.
    pub const IDLE: Self = Self {
        load: 0.0,
        peak: 0.0,
        overruns: 0,
    };

    /// How many frames the recent window spans.
    ///
    /// A second's worth. The reader is a person looking at a page rather than
    /// another loop, so the window is what a spike has to survive to be seen at
    /// all: a maximum decaying over two frames is gone in 66 ms.
    pub const RECENT_FRAMES: usize = FRAMES_IN_A_SECOND;

    /// The fraction of the budget the worst recent frame left unused.
    ///
    /// Negative where that frame overran rather than clamped at zero: how far
    /// past the deadline it went is the difference between drawing that is
    /// slightly too slow and drawing that is hopelessly too slow.
    ///
    /// ```
    /// use motif::ui::Pace;
    ///
    /// assert_eq!(Pace::IDLE.spare(), 1.0);
    /// ```
    pub fn spare(self) -> f32 {
        1.0 - self.peak
    }
}

/// Build a frame meter, and split it into the measuring end and the reading
/// end.
///
/// The storage is allocated here and never again, so this belongs in setup.
/// The measuring end goes to the [`EventLoop`](crate::ui::EventLoop) through
/// [`metering`](crate::ui::EventLoop::metering), the reading end to whatever
/// draws it.
///
/// ```
/// use motif::device::DeviceProfile;
/// use motif::ui::pace_meter;
///
/// let budget = DeviceProfile::TARGET.screen.frame_budget();
/// let (mut writer, reader) = pace_meter();
///
/// writer.measured(budget, 0);
///
/// assert_eq!(reader.read().load, 1.0);
/// assert_eq!(reader.read().spare(), 0.0);
/// ```
pub fn pace_meter() -> (PaceWriter, PaceReader) {
    let published = Rc::new(Cell::new(Pace::IDLE));

    (
        PaceWriter {
            published: Rc::clone(&published),
            window: Window::spanning(Pace::RECENT_FRAMES),
        },
        PaceReader { published },
    )
}

/// The measuring end of a frame meter, held by the loop.
pub struct PaceWriter {
    published: Rc<Cell<Pace>>,
    window: Window,
}

impl PaceWriter {
    /// Publish a frame that spent `elapsed` of its budget, in a run that has
    /// overrun `overruns` times, and report what was published.
    ///
    /// The overrun count arrives from the loop's own counter rather than being
    /// kept here, so what a page reads mid-run and what the run reports at the
    /// end are one number and cannot drift apart.
    pub fn measured(&mut self, elapsed: Duration, overruns: u64) -> Pace {
        let load = (elapsed.as_nanos() as f64 / BUDGET.as_nanos() as f64) as f32;

        let pace = Pace {
            load,
            peak: self.window.holding(load),
            overruns,
        };
        self.published.set(pace);
        pace
    }
}

/// The reading end of a frame meter, held by whatever reports it.
pub struct PaceReader {
    published: Rc<Cell<Pace>>,
}

impl PaceReader {
    /// The most recent frame's reading, or [`Pace::IDLE`] where no frame has
    /// been measured yet.
    ///
    /// Reading takes nothing: a peak stays readable until the window it belongs
    /// to has passed, so looking twice in a frame reports it twice rather than
    /// the second look finding it gone. The window advances with the frames
    /// that are measured rather than with the clock, so a loop that has stopped
    /// keeps reporting the window it stopped in.
    pub fn read(&self) -> Pace {
        self.published.get()
    }
}

const BUDGET: Duration = DeviceProfile::TARGET.screen.frame_budget();
