//! The loop that runs an application: controls in, a frame out, at a fixed rate.
//!
//! The rate is [`ScreenProfile::frame_budget`], and it is a budget rather than a
//! target to beat. Drawing as often as the machine allows costs a laptop
//! nothing and costs the target the time analysis needs, so a frame that
//! finishes early gives the rest back.
//!
//! An [`App`] is handed controls and a blank [`Region`], and says whether it is
//! still running. It never learns what the frame is drawn on or what the player
//! touched to produce a control, which is what keeps a terminal one backend
//! among others.
//!
//! A control is marked as it is handed over and the mark aged once the frame is
//! shown, so the panel says an event arrived whether or not the application did
//! anything with it.
//!
//! ```
//! use motif::device::Button;
//! use motif::ui::{
//!     App, Cell, ControlEvent, EventLoop, Flow, Legend, NullRenderer, Region, ScriptedControls,
//! };
//!
//! struct Splash;
//!
//! impl App for Splash {
//!     fn control(&mut self, event: ControlEvent) -> Flow {
//!         match event {
//!             ControlEvent::Pressed { button: Button::Stop, .. } => Flow::Exit,
//!             _ => Flow::Continue,
//!         }
//!     }
//!
//!     fn legend(&self) -> Legend {
//!         Legend::blank().answering(Button::Stop)
//!     }
//!
//!     fn draw(&mut self, mut region: Region<'_>) -> Flow {
//!         region.set(0, 0, Cell::new('m'));
//!         Flow::Continue
//!     }
//! }
//!
//! let mut app = Splash;
//! let mut controls = ScriptedControls::new([ControlEvent::Pressed {
//!     button: Button::Stop,
//!     shifted: false,
//! }]);
//! let mut screen = NullRenderer::new();
//!
//! let report = EventLoop::new().run(&mut app, &mut controls, &mut screen)?;
//!
//! assert_eq!(report.frames(), 0);
//! # Ok::<(), motif::ui::RenderError>(())
//! ```

use std::time::Duration;

use crate::device::DeviceProfile;
#[cfg(feature = "frame-pace")]
use crate::ui::PaceWriter;
use crate::ui::{
    Clock, ControlEvent, Controls, Frame, Legend, Marks, Region, RenderError, Renderer, SystemClock,
};

/// The most control events one frame will take.
///
/// A bound rather than "whatever is waiting", because a panel is not obliged to
/// run dry: a terminal handed a pasted page of text produces a control per
/// character for as long as the paste lasts, and a drain that read until it
/// stopped would spend the frame reading instead of drawing. The panel has
/// a dozen controls and a player has two hands, so this is far more than one
/// frame of playing; reaching it means the source is not a player, and what is
/// left over waits for the next frame rather than being dropped.
pub const EVENTS_PER_FRAME: usize = 32;

/// Whether the application is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Carry on to the next frame.
    Continue,
    /// Stop, and hand control back to whoever started the loop.
    Exit,
}

impl Flow {
    /// Whether this is [`Flow::Exit`].
    pub const fn is_exit(self) -> bool {
        matches!(self, Self::Exit)
    }
}

/// An application an [`EventLoop`] can run.
///
/// Taking a control and drawing both answer with a [`Flow`], because a run ends
/// for more reasons than a player pressing something: a state the application
/// reaches between frames can end it too.
pub trait App {
    /// Take one thing the player did.
    ///
    /// Called once per event, for every event waiting when the frame began.
    fn control(&mut self, event: ControlEvent) -> Flow;

    /// Which controls this application answers, and what each one does here.
    ///
    /// Required rather than defaulted, because a control answered without being
    /// declared is exactly what the legend exists to stop: a page that says
    /// nothing has decided to say nothing, instead of having forgotten to.
    fn legend(&self) -> Legend;

    /// Put the application's state on `region`.
    ///
    /// The region arrives blank. Drawing is what the frame budget is for, so
    /// this is the one place in a frame where an application is expected to
    /// spend it.
    ///
    /// Every cell of the region is the application's, and nothing is drawn over
    /// it afterwards: an application adding chrome takes rows off the region
    /// first and hands the rest to whatever it wraps.
    fn draw(&mut self, region: Region<'_>) -> Flow;
}

/// What a run of the loop did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunReport {
    frames: u64,
    overruns: u64,
}

impl RunReport {
    /// How many frames reached the screen.
    pub const fn frames(self) -> u64 {
        self.frames
    }

    /// How many of them took longer than the budget allows.
    ///
    /// An overrun is reported rather than made up for: a loop that ran the next
    /// frame early to catch up would draw two frames back to back and spend
    /// twice the budget doing it, which is the opposite of what a budget is
    /// for.
    pub const fn overruns(self) -> u64 {
        self.overruns
    }
}

/// The loop an application runs inside.
///
/// It owns the [`Frame`] every draw goes into and overwrites it with a blank
/// one between draws, so the loop itself allocates nothing per frame. What a
/// backend does with the frame it is handed is its own affair: the terminal's
/// [`FrameWriter`](crate::ui::FrameWriter) does allocate per frame, and the
/// bound this loop keeps is on its own work.
pub struct EventLoop<K: Clock = SystemClock> {
    clock: K,
    budget: Duration,
    frame: Frame,
    marks: Marks,
    #[cfg(feature = "frame-pace")]
    pace: Option<PaceWriter>,
}

impl EventLoop<SystemClock> {
    /// A loop paced by the machine's clock.
    pub fn new() -> Self {
        Self::with_clock(SystemClock::new())
    }
}

impl Default for EventLoop<SystemClock> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Clock> EventLoop<K> {
    /// A loop paced by `clock`.
    pub fn with_clock(clock: K) -> Self {
        Self {
            clock,
            budget: DeviceProfile::TARGET.screen.frame_budget(),
            frame: Frame::blank(),
            marks: Marks::none(),
            #[cfg(feature = "frame-pace")]
            pace: None,
        }
    }

    /// The clock the loop is pacing itself against.
    pub fn clock(&self) -> &K {
        &self.clock
    }

    /// Publish what each frame cost to `pace`.
    ///
    /// A loop given no meter measures nothing, which is what makes the two
    /// clock reads it already takes the whole cost of this: the readings are
    /// taken for the budget either way.
    ///
    /// A meter belongs to one run. Its recent window advances with the frames
    /// it is given and nothing resets it, so running the same loop again would
    /// open with a peak the previous run left behind.
    #[cfg(feature = "frame-pace")]
    pub fn metering(mut self, pace: PaceWriter) -> Self {
        self.pace = Some(pace);
        self
    }

    /// Run `app` until it asks to stop, drawing to `screen` and reading `controls`.
    ///
    /// A frame takes up to [`EVENTS_PER_FRAME`] control events, draws, renders,
    /// hands the screen the panel the [`Legend`] makes, and waits out its budget.
    /// The picture is made here, the only place holding both halves of it.
    ///
    /// An exit from [`App::control`] ends the run undrawn; one from [`App::draw`]
    /// renders that frame first. Neither waits out its budget.
    ///
    /// # Errors
    ///
    /// Returns the [`RenderError`] the screen gave, having stopped.
    pub fn run(
        &mut self,
        app: &mut impl App,
        controls: &mut impl Controls,
        screen: &mut impl Renderer,
    ) -> Result<RunReport, RenderError> {
        let mut report = RunReport::default();

        loop {
            let started = self.clock.now();

            if drain(app, controls, &mut self.marks).is_exit() || controls.interrupted() {
                return Ok(report);
            }

            self.frame = Frame::blank();
            let flow = app.draw(self.frame.region());
            screen.render(&self.frame)?;
            screen.show_panel(&app.legend().picture(controls, self.marks))?;
            self.marks.age();
            report.frames += 1;

            let spent = self.clock.now().duration_since(started);
            let spare = self.budget.checked_sub(spent);
            if spare.is_none() {
                report.overruns += 1;
            }
            self.measured(spent, report.overruns);

            if flow.is_exit() {
                return Ok(report);
            }

            if let Some(spare) = spare {
                self.clock.sleep(spare);
            }
        }
    }

    #[cfg(feature = "frame-pace")]
    fn measured(&mut self, spent: Duration, overruns: u64) {
        if let Some(pace) = &mut self.pace {
            pace.measured(spent, overruns);
        }
    }

    #[cfg(not(feature = "frame-pace"))]
    fn measured(&mut self, _spent: Duration, _overruns: u64) {}
}

fn drain(app: &mut impl App, controls: &mut impl Controls, marks: &mut Marks) -> Flow {
    for _ in 0..EVENTS_PER_FRAME {
        let Some(event) = controls.poll() else {
            break;
        };

        marks.fired(event);

        if app.control(event).is_exit() {
            return Flow::Exit;
        }
    }

    Flow::Continue
}
