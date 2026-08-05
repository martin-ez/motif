//! Holding a device open for the length of a run, and showing what it is doing.
//!
//! [`Monitor`] is the half of the composition that holds the device and not the
//! decision: it opens whatever a backend would open if nobody chose, starts it,
//! keeps it for the run and stops it on the way out, while what its streams play
//! is the caller's to say. It wraps an [`App`] rather than being one, so the
//! pages under it neither know nor need to know that a device is open — the
//! bottom row is taken for the state before the pages are handed the rest, and
//! every cell they are given stays theirs.
//!
//! A device that is not there is drawn, never fatal. An instrument that quit
//! because an interface was unplugged would be wrong, and one that drew a
//! plausible idle screen over a dead audio path would be worse, so the state
//! goes on the frame and the run carries on.

use crate::audio::{AudioBackend, AudioPath, AudioState, DeviceError, DeviceLink, StreamRequest};
use crate::ui::{App, ControlEvent, Flow, Legend, Region};

const LABEL: &str = "audio ";
const STATUS_ROWS: usize = 1;

/// An application with an audio device held open behind it.
///
/// Everything it does with the device happens on the application thread, which
/// is where opening, starting, stopping and dropping a stream belong. What goes
/// on the callback is whatever the caller's `path` builds, and a monitor is
/// what keeps that stream alive long enough to hear.
///
/// ```
/// use motif::audio::{AudioState, NullBackend, Passthrough, StreamConfig, StreamRequest};
/// use motif::device::Button;
/// use motif::monitor::Monitor;
/// use motif::ui::{App, ControlEvent, Flow, Legend, Region};
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
///     fn draw(&mut self, _region: Region<'_>) -> Flow {
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
/// let monitor = Monitor::opened(
///     Quiet,
///     NullBackend::rounding(granted),
///     request,
///     Passthrough::new,
/// );
///
/// assert_eq!(monitor.state(), AudioState::Playing);
/// ```
pub struct Monitor<A: App, B: AudioBackend, F> {
    app: A,
    link: Option<DeviceLink<B, F>>,
}

impl<A: App, B: AudioBackend, F, P> Monitor<A, B, F>
where
    F: FnMut() -> P,
    P: AudioPath,
{
    /// Open what `backend` would open at `request` if nobody chose, start it
    /// playing through what `path` builds, and hold it behind `app`.
    ///
    /// This cannot fail. A backend with nothing to offer, one that refuses the
    /// request and one whose device will not start all leave the monitor in
    /// [`AudioState::Lost`] carrying why, that being something to draw rather
    /// than a reason not to run. A backend with nothing to offer never builds a
    /// path: there is no stream for one to play through.
    pub fn opened(app: A, backend: B, request: StreamRequest, path: F) -> Self {
        let Some(selection) = backend.defaults(request.sample_rate) else {
            return Self { app, link: None };
        };

        let mut link = DeviceLink::new(backend, request, selection, path);
        if link.open().is_ok() {
            let _started = link.start();
        }

        Self {
            app,
            link: Some(link),
        }
    }
}

impl<A: App, B: AudioBackend, F> Monitor<A, B, F> {
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
    pub fn link(&self) -> Option<&DeviceLink<B, F>> {
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

impl<A: App, B: AudioBackend, F> App for Monitor<A, B, F> {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.app.control(event)
    }

    fn legend(&self) -> Legend {
        self.app.legend()
    }

    /// The bottom row is taken for the state before the wrapped application is
    /// handed the rest, so a page filling what it was given cannot land on it.
    /// The application's [`Flow`] is the one that comes back, so nothing about
    /// the audio path can end a run. The device is polled once here, which is
    /// where a fault the callback latched is noticed.
    fn draw(&mut self, region: Region<'_>) -> Flow {
        let (above, mut status) = region.split_bottom(STATUS_ROWS);

        let flow = self.app.draw(above);
        let state = self.polled();

        status.write(0, 0, &format!("{LABEL}{state}"));

        flow
    }
}

impl<A: App, B: AudioBackend, F> Drop for Monitor<A, B, F> {
    fn drop(&mut self) {
        self.close();
    }
}
