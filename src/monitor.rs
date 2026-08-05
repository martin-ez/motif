//! Holding a device open for the length of a run, and showing what it is doing.
//!
//! [`Monitor`] is the half of the composition that has no engine in it: it
//! opens whatever a backend would open if nobody chose, starts it, keeps it for
//! the run and stops it on the way out. It wraps an [`App`] rather than being
//! one, so the pages under it neither know nor need to know that a device is
//! open — what they draw is theirs, and the state is added beneath it.
//!
//! A device that is not there is drawn, never fatal. An instrument that quit
//! because an interface was unplugged would be wrong, and one that drew a
//! plausible idle screen over a dead audio path would be worse, so the state
//! goes on the frame and the run carries on.

use crate::audio::{AudioBackend, AudioState, DeviceError, DeviceLink, Passthrough, StreamRequest};
use crate::device::DeviceProfile;
use crate::ui::{App, Cell, ControlEvent, Flow, Frame, Legend};

const LABEL: &str = "audio ";

type MonitorLink<B> = DeviceLink<B, fn() -> Passthrough>;

fn status_row() -> usize {
    DeviceProfile::TARGET.screen.rows.saturating_sub(2)
}

fn write(frame: &mut Frame, row: usize, text: &str) {
    for (column, glyph) in text.chars().enumerate() {
        frame.set(column, row, Cell::new(glyph));
    }
}

/// An application with an audio device held open behind it.
///
/// Everything it does with the device happens on the application thread, which
/// is where opening, starting, stopping and dropping a stream belong. What it
/// puts on the callback is [`Passthrough`] and nothing else, and a monitor is
/// what keeps that stream alive long enough to hear.
///
/// ```
/// use motif::audio::{AudioState, NullBackend, StreamConfig, StreamRequest};
/// use motif::device::Button;
/// use motif::monitor::Monitor;
/// use motif::ui::{App, ControlEvent, Flow, Frame, Legend};
///
/// struct Quiet;
///
/// impl App for Quiet {
///     fn control(&mut self, _event: ControlEvent) -> Flow {
///         Flow::Continue
///     }
///
///     fn legend(&self) -> Legend {
///         Legend::blank().answering(Button::Play)
///     }
///
///     fn draw(&mut self, _frame: &mut Frame) -> Flow {
///         Flow::Continue
///     }
/// }
///
/// let granted = StreamConfig {
///     sample_rate: 48_000,
///     block_size: 256,
///     input_channels: 2,
///     output_channels: 2,
/// };
/// let request = StreamRequest {
///     sample_rate: 48_000,
///     block_size: 256,
/// };
///
/// let monitor = Monitor::opened(Quiet, NullBackend::rounding(granted), request);
///
/// assert_eq!(monitor.state(), AudioState::Playing);
/// ```
pub struct Monitor<A: App, B: AudioBackend> {
    app: A,
    link: Option<MonitorLink<B>>,
}

impl<A: App, B: AudioBackend> Monitor<A, B> {
    /// Open what `backend` would open at `request` if nobody chose, start it,
    /// and hold it behind `app`.
    ///
    /// This cannot fail. A backend with nothing to offer, one that refuses the
    /// request and one whose device will not start all leave the monitor in
    /// [`AudioState::Lost`] carrying why, that being something to draw rather
    /// than a reason not to run.
    pub fn opened(app: A, backend: B, request: StreamRequest) -> Self {
        let Some(selection) = backend.defaults(request.sample_rate) else {
            return Self { app, link: None };
        };

        let mut link = DeviceLink::new(backend, request, selection, Passthrough::new as fn() -> _);
        if link.open().is_ok() {
            let _started = link.start();
        }

        Self {
            app,
            link: Some(link),
        }
    }

    /// What the audio path is doing, as of the last frame drawn.
    ///
    /// A backend that had no device to open reports
    /// [`DeviceError::DeviceNotAvailable`], one offering nothing being what a
    /// player cannot tell apart from a device that has gone.
    pub fn state(&self) -> AudioState {
        match &self.link {
            Some(link) => link.state(),
            None => AudioState::Lost(DeviceError::DeviceNotAvailable),
        }
    }

    /// The link holding the stream, or `None` where there was no device to
    /// open.
    ///
    /// This is the route to what the stream knows and the monitor does not: the
    /// configuration the device granted, the levels, the dropout counts, the
    /// callback's headroom.
    pub fn link(&self) -> Option<&MonitorLink<B>> {
        self.link.as_ref()
    }

    /// Stop and drop the stream, leaving the application running.
    ///
    /// What dropping the monitor does, so that a run can hand the device back
    /// before the monitor itself goes.
    pub fn close(&mut self) {
        if let Some(link) = self.link.as_mut() {
            link.close();
        }
    }

    fn polled(&mut self) -> AudioState {
        if let Some(link) = self.link.as_mut() {
            link.poll();
        }

        self.state()
    }
}

impl<A: App, B: AudioBackend> App for Monitor<A, B> {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.app.control(event)
    }

    fn legend(&self) -> Legend {
        self.app.legend()
    }

    /// The wrapped application draws first and its [`Flow`] is the one that
    /// comes back, so nothing about the audio path can end a run. The device is
    /// polled once here, which is where a fault the callback latched is
    /// noticed, and the state lands beneath whatever was drawn.
    fn draw(&mut self, frame: &mut Frame) -> Flow {
        let flow = self.app.draw(frame);
        let state = self.polled();

        write(frame, status_row(), &format!("{LABEL}{state}"));

        flow
    }
}

impl<A: App, B: AudioBackend> Drop for Monitor<A, B> {
    fn drop(&mut self) {
        self.close();
    }
}
