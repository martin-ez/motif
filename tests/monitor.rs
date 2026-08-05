//! Holding a device open for the length of a run, and what a player sees when
//! there is no device to hold.
//!
//! The wrapped application here is the test's own, because what a monitor wraps
//! is a composition question and not something the monitor knows. It counts the
//! frames it drew and ends the run on the frame a test asks for, so a run
//! against a device that never opened is something a test can watch finish.
//!
//! The backend with no hardware behind it covers everything but the stopping,
//! which leaves no trace once the stream is dropped. That one gets a backend of
//! its own, recording what was asked of the stream where the monitor that held
//! it is already gone.

use std::cell::RefCell;
use std::rc::Rc;

use motif::audio::{
    AudioBackend, AudioDevice, AudioHost, AudioState, ChannelSelection, DeviceError,
    DeviceSelection, DuplexStream, Headroom, Levels, NullBackend, StreamConfig, StreamRequest,
    StreamState, Xruns,
};
use motif::device::{Button, DeviceProfile};
use motif::monitor::Monitor;
use motif::ui::{
    App, Cell, ControlEvent, EventLoop, Flow, Frame, Legend, NullRenderer, RunReport,
    ScriptedClock, ScriptedControls,
};

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 256,
        input_channels: 2,
        output_channels: 2,
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: 48_000,
        block_size: 256,
    }
}

fn other_request() -> StreamRequest {
    StreamRequest {
        block_size: 64,
        ..request()
    }
}

fn deaf() -> StreamConfig {
    StreamConfig {
        input_channels: 0,
        ..config()
    }
}

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

/// What a wrapped application was handed, readable while the monitor holds it.
#[derive(Clone, Default)]
struct Taken(Rc<RefCell<Vec<ControlEvent>>>);

impl Taken {
    fn events(&self) -> Vec<ControlEvent> {
        self.0.borrow().clone()
    }
}

/// An application that draws a glyph, keeps what it was handed, and ends the
/// run on a frame the test picks.
#[derive(Default)]
struct Counted {
    taken: Taken,
    drawn: u32,
    lasts: u32,
}

impl Counted {
    fn lasting(frames: u32) -> Self {
        Self {
            lasts: frames,
            ..Self::default()
        }
    }
}

impl App for Counted {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.taken.0.borrow_mut().push(event);

        Flow::Continue
    }

    fn legend(&self) -> Legend {
        Legend::blank().answering(Button::Record)
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        self.drawn += 1;
        frame.set(0, 0, Cell::new('m'));

        if self.lasts > 0 && self.drawn >= self.lasts {
            return Flow::Exit;
        }

        Flow::Continue
    }
}

/// What a stream was asked to do, readable after the stream is gone.
#[derive(Clone, Default)]
struct Asked(Rc<RefCell<Vec<&'static str>>>);

impl Asked {
    fn of(&self) -> Vec<&'static str> {
        self.0.borrow().clone()
    }

    fn record(&self, what: &'static str) {
        self.0.borrow_mut().push(what);
    }
}

struct RecordingBackend(Asked);

struct RecordingStream(Asked);

impl AudioBackend for RecordingBackend {
    type Stream = RecordingStream;

    fn hosts(&self, _sample_rate: u32) -> Vec<AudioHost> {
        vec![AudioHost {
            name: "recording".to_owned(),
            inputs: vec![AudioDevice {
                name: "in".to_owned(),
                channels: vec![2],
            }],
            outputs: vec![AudioDevice {
                name: "out".to_owned(),
                channels: vec![2],
            }],
        }]
    }

    fn defaults(&self, _sample_rate: u32) -> Option<DeviceSelection> {
        Some(DeviceSelection {
            host: "recording".to_owned(),
            input: "in".to_owned(),
            input_channels: ChannelSelection::all(2),
            output: "out".to_owned(),
            output_channels: ChannelSelection::all(2),
        })
    }

    fn open(
        &self,
        _selection: &DeviceSelection,
        _request: StreamRequest,
    ) -> Result<Self::Stream, DeviceError> {
        self.0.record("open");

        Ok(RecordingStream(self.0.clone()))
    }
}

impl DuplexStream for RecordingStream {
    fn config(&self) -> StreamConfig {
        config()
    }

    fn state(&self) -> StreamState {
        StreamState::Stopped
    }

    fn levels(&self) -> Levels {
        Levels::SILENT
    }

    fn xruns(&self) -> Xruns {
        Xruns::NONE
    }

    fn headroom(&self) -> Headroom {
        Headroom::IDLE
    }

    fn fault(&self) -> Option<DeviceError> {
        None
    }

    fn start(&mut self) -> Result<(), DeviceError> {
        self.0.record("start");

        Ok(())
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        self.0.record("stop");

        Ok(())
    }
}

fn monitoring(
    app: Counted,
    backend: NullBackend,
    request: StreamRequest,
) -> Monitor<Counted, NullBackend> {
    Monitor::opened(app, backend, request)
}

fn playing() -> Monitor<Counted, NullBackend> {
    monitoring(
        Counted::default(),
        NullBackend::rounding(config()),
        request(),
    )
}

fn unplug(monitor: &Monitor<Counted, NullBackend>) {
    monitor
        .link()
        .expect("a monitor over a device that opened has a link")
        .stream()
        .expect("an open link has a stream")
        .fail(DeviceError::DeviceNotAvailable);
}

fn drawn(monitor: &mut Monitor<Counted, NullBackend>) -> Frame {
    let mut frame = Frame::blank();
    monitor.draw(&mut frame);

    frame
}

fn status(frame: &Frame) -> String {
    let row = DeviceProfile::TARGET.screen.rows - 2;

    (0..DeviceProfile::TARGET.screen.columns)
        .filter_map(|column| frame.get(column, row))
        .map(Cell::glyph)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn run(monitor: &mut Monitor<Counted, NullBackend>) -> RunReport {
    EventLoop::with_clock(ScriptedClock::new([]))
        .run(
            monitor,
            &mut ScriptedControls::new([]),
            &mut NullRenderer::new(),
        )
        .expect("a null renderer never fails")
}

#[test]
fn a_monitor_opens_the_default_device_and_starts_it() {
    assert_eq!(playing().state(), AudioState::Playing);
}

#[test]
fn a_backend_with_no_default_device_leaves_the_monitor_lost() {
    let monitor = monitoring(Counted::default(), NullBackend::rounding(deaf()), request());

    assert_eq!(
        monitor.state(),
        AudioState::Lost(DeviceError::DeviceNotAvailable)
    );
}

#[test]
fn a_backend_with_no_default_device_has_no_link() {
    let monitor = monitoring(Counted::default(), NullBackend::rounding(deaf()), request());

    assert!(monitor.link().is_none());
}

#[test]
fn a_device_that_refuses_to_open_leaves_the_monitor_lost() {
    let monitor = monitoring(
        Counted::default(),
        NullBackend::rejecting(config()),
        other_request(),
    );

    assert_eq!(
        monitor.state(),
        AudioState::Lost(DeviceError::UnsupportedConfig)
    );
}

#[test]
fn a_device_that_goes_away_is_noticed_on_the_next_frame() {
    let mut monitor = playing();
    unplug(&monitor);

    drawn(&mut monitor);

    assert_eq!(
        monitor.state(),
        AudioState::Lost(DeviceError::DeviceNotAvailable)
    );
}

#[test]
fn a_device_that_never_opened_does_not_end_the_run() {
    let mut monitor = monitoring(
        Counted::lasting(3),
        NullBackend::rejecting(config()),
        other_request(),
    );

    assert_eq!(run(&mut monitor).frames(), 3);
}

#[test]
fn a_device_that_goes_away_does_not_end_the_run() {
    let mut monitor = monitoring(
        Counted::lasting(3),
        NullBackend::rounding(config()),
        request(),
    );
    unplug(&monitor);

    assert_eq!(run(&mut monitor).frames(), 3);
}

#[test]
fn a_playing_device_is_drawn_where_the_player_can_see_it() {
    let mut monitor = playing();

    assert_eq!(status(&drawn(&mut monitor)), "audio playing");
}

#[test]
fn a_lost_device_says_on_screen_why_it_was_lost() {
    let mut monitor = playing();
    unplug(&monitor);

    assert_eq!(
        status(&drawn(&mut monitor)),
        "audio lost: the device is not available"
    );
}

#[test]
fn the_wrapped_application_still_draws() {
    let mut monitor = playing();

    assert_eq!(drawn(&mut monitor).get(0, 0), Some(Cell::new('m')));
}

#[test]
fn controls_reach_the_wrapped_application() {
    let app = Counted::default();
    let taken = app.taken.clone();
    let mut monitor = monitoring(app, NullBackend::rounding(config()), request());

    monitor.control(pressed(Button::Record));

    assert_eq!(taken.events(), [pressed(Button::Record)]);
}

#[test]
fn the_legend_is_the_wrapped_applications() {
    let monitor = playing();

    assert!(monitor.legend().answers(Button::Record));
    assert!(!monitor.legend().answers(Button::Play));
}

#[test]
fn the_wrapped_application_still_ends_the_run() {
    let mut monitor = monitoring(
        Counted::lasting(1),
        NullBackend::rounding(config()),
        request(),
    );

    assert_eq!(run(&mut monitor).frames(), 1);
}

#[test]
fn closing_a_monitor_closes_the_device() {
    let mut monitor = playing();

    monitor.close();

    assert_eq!(monitor.state(), AudioState::Closed);
}

#[test]
fn a_closed_monitor_says_so_on_screen() {
    let mut monitor = playing();
    monitor.close();

    assert_eq!(status(&drawn(&mut monitor)), "audio closed");
}

#[test]
fn a_monitor_stops_the_stream_when_the_run_ends() {
    let asked = Asked::default();

    drop(Monitor::opened(
        Counted::default(),
        RecordingBackend(asked.clone()),
        request(),
    ));

    assert_eq!(asked.of(), ["open", "start", "stop"]);
}
