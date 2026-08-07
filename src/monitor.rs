//! Holding a device open for the length of a run, and showing what it is doing.
//!
//! [`Monitor`] is the half of the composition that holds the device and not the
//! decision: it starts the link it is lent, keeps it for the run and stops it on
//! the way out, while what its streams play is the caller's to say. It wraps an
//! [`App`] rather than being one, so the pages under it need not know a device
//! is open: the bottom row is taken before they are handed the rest.
//!
//! That row carries two levels, not one: a meter on the input alone reads the
//! converter, which a muted engine and a stopped callback both leave moving.
//!
//! A device that is not there is drawn, never fatal. An instrument that quit
//! because an interface was unplugged would be wrong, and one that drew a
//! plausible idle screen over a dead audio path would be worse, so the state
//! goes on the frame and the run carries on.

use crate::audio::{
    AudioBackend, AudioPath, AudioState, DeviceError, DeviceLink, DuplexStream, Levels, SharedLink,
};
use crate::ui::{App, ControlEvent, Flow, LevelMeter, Region};

const LABEL: &str = "audio ";
const STATUS_ROWS: usize = 1;
const METER_COLUMNS: usize = 20;
const CAPTURED_LABEL: &str = "in ";
const PLAYED_LABEL: &str = " out ";
const METERS_COLUMNS: usize =
    CAPTURED_LABEL.len() + PLAYED_LABEL.len() + METER_COLUMNS + METER_COLUMNS;

/// An application with an audio device held open behind it.
///
/// Everything it does with the device happens on the application thread, which
/// is where opening, starting, stopping and dropping a stream belong. What goes
/// on the callback is whatever the link's path builds, and a monitor is what
/// keeps that stream alive long enough to hear.
///
/// ```
/// use motif::audio::{
///     AudioState, NullBackend, Passthrough, SharedLink, StreamConfig, StreamRequest,
/// };
/// use motif::device::Button;
/// use motif::monitor::Monitor;
/// use motif::ui::{App, ControlEvent, Flow, Region};
///
/// struct Quiet;
///
/// impl App for Quiet {
///     fn control(&mut self, _event: ControlEvent) -> Flow {
///         Flow::Continue
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
/// let link = SharedLink::defaulting(NullBackend::rounding(granted), request, Passthrough::new);
///
/// let monitor = Monitor::watching(Quiet, link);
///
/// assert_eq!(monitor.state(), AudioState::Playing);
/// ```
pub struct Monitor<A: App, B: AudioBackend, F> {
    app: A,
    link: Option<SharedLink<B, F>>,
    captured: LevelMeter,
    played: LevelMeter,
}

impl<A: App, B: AudioBackend, F, P> Monitor<A, B, F>
where
    F: Fn() -> P + Send + Sync + 'static,
    P: AudioPath,
{
    /// Open `link`, start it playing, and hold it for the run behind `app`.
    ///
    /// The link belongs to the run rather than to the monitor, so whatever else
    /// configures it holds a handle of its own and a composition with no device
    /// to open passes `None`. Opening happens here rather than where the link
    /// was built, so that a page listing what it could be opened on has done so
    /// before any stream holds the device.
    ///
    /// This cannot fail. A device that refuses the request or will not start
    /// leaves the monitor in [`AudioState::Lost`] carrying why.
    ///
    /// The one open a run waits for, rather than taking the answer at a later
    /// [`DeviceLink::poll`]: there is no frame to hand back before the first
    /// one is drawn, and a run that started silently while its device was still
    /// opening would draw a screen nobody could play into.
    pub fn watching(app: A, mut link: Option<SharedLink<B, F>>) -> Self {
        if let Some(link) = link.as_mut() {
            link.change(|held| {
                held.open();
                held.settled();
                let _started = held.start();
            });
        }

        Self {
            app,
            link,
            captured: LevelMeter::new(),
            played: LevelMeter::new(),
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
            Some(link) => link.read(DeviceLink::state),
            None => AudioState::Lost(DeviceError::DeviceNotAvailable),
        }
    }

    /// A handle on the link holding the stream, or `None` where there was no
    /// device to open.
    ///
    /// This is the route to what the stream knows and the monitor does not: the
    /// configuration the device granted, the levels, the dropout counts, the
    /// callback's headroom.
    pub fn link(&self) -> Option<&SharedLink<B, F>> {
        self.link.as_ref()
    }

    /// Stop and drop the stream, leaving the application running.
    ///
    /// What dropping the monitor does, so that a run can hand the device back
    /// before the monitor itself goes. Whatever else holds the link keeps its
    /// handle, and finds it closed.
    pub fn close(&mut self) {
        if let Some(link) = self.link.as_mut() {
            link.change(DeviceLink::close);
        }
    }

    fn metered(&self, of: fn(&B::Stream) -> Levels) -> Levels {
        self.link.as_ref().map_or(Levels::SILENT, |link| {
            link.read(|held| held.stream().map_or(Levels::SILENT, of))
        })
    }

    fn bars(&mut self, state: AudioState) -> Option<String> {
        if let AudioState::Lost(_) = state {
            return None;
        }

        let captured = self.metered(DuplexStream::captured);
        let played = self.metered(DuplexStream::played);

        Some(format!(
            "{CAPTURED_LABEL}{}{PLAYED_LABEL}{}",
            self.captured.bar(captured, METER_COLUMNS),
            self.played.bar(played, METER_COLUMNS)
        ))
    }

    fn polled(&mut self) -> AudioState {
        if let Some(link) = self.link.as_mut() {
            link.change(DeviceLink::poll);
        }

        self.state()
    }
}

impl<A: App, B: AudioBackend, F> App for Monitor<A, B, F> {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.app.control(event)
    }

    /// The bottom row is taken for the state and the two levels before the
    /// wrapped application is handed the rest, so a page filling what it was
    /// given cannot land on it. The application's [`Flow`] is the one that comes
    /// back, so nothing about the audio path can end a run. The device is polled
    /// once here, which is where a fault the callback latched is noticed.
    ///
    /// A lost device gets the row to itself: the labelled pair does not fit
    /// beside why it went, and a stream that is gone has no level to report.
    fn draw(&mut self, region: Region<'_>) -> Flow {
        let (above, mut status) = region.split_bottom(STATUS_ROWS);

        let flow = self.app.draw(above);
        let state = self.polled();
        let margin = status.columns().saturating_sub(METERS_COLUMNS);

        status.write(0, 0, &format!("{LABEL}{state}"));
        if let Some(bars) = self.bars(state) {
            status.write(margin, 0, &bars);
        }

        flow
    }
}

impl<A: App, B: AudioBackend, F> Drop for Monitor<A, B, F> {
    fn drop(&mut self) {
        self.close();
    }
}
