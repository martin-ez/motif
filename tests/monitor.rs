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
use std::sync::{Arc, Mutex};

use motif::audio::{
    AudioBackend, AudioDevice, AudioHost, AudioPath, AudioState, ChannelSelection, Command,
    DeviceError, DeviceId, DeviceSelection, DuplexStream, Headroom, Levels, NullBackend,
    Passthrough, StreamConfig, StreamRequest, StreamState, Xruns,
};
use motif::device::{Button, DeviceProfile};
use motif::monitor::Monitor;
use motif::ui::{
    App, Cell, ControlEvent, EventLoop, Flow, Frame, Legend, NullRenderer, Region, RunReport,
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

const FILL: char = '#';

/// How wide the monitor draws the level meter, and where its left edge lands.
///
/// The monitor keeps it against the right margin so that the state it writes
/// from column zero has the rest of the row, which is what these say.
const METER_COLUMNS: usize = 24;
const METER_COLUMN: usize = DeviceProfile::TARGET.screen.columns - METER_COLUMNS;
const METER_SCALE: usize = METER_COLUMNS - 2;

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

    fn draw(&mut self, mut region: Region<'_>) -> Flow {
        self.drawn += 1;
        region.set(0, 0, Cell::new('m'));

        if self.lasts > 0 && self.drawn >= self.lasts {
            return Flow::Exit;
        }

        Flow::Continue
    }
}

/// An application that writes into every cell of the region it is handed.
///
/// A page that fills what it was given is the case the monitor's status row used
/// to be safe from by luck, so it is the one that says the region is a region.
struct Filling;

impl App for Filling {
    fn control(&mut self, _event: ControlEvent) -> Flow {
        Flow::Continue
    }

    fn legend(&self) -> Legend {
        Legend::blank().answering(Button::Record)
    }

    fn draw(&mut self, mut region: Region<'_>) -> Flow {
        for row in 0..region.rows() {
            for column in 0..region.columns() {
                region.set(column, row, Cell::new(FILL));
            }
        }

        Flow::Continue
    }
}

/// A path that keeps what it was prepared with, readable once the monitor has
/// moved it into a stream.
///
/// A monitor lends its stream out, so what the callback plays cannot be read
/// back through one. What the path was prepared with can, and it is only
/// prepared where a stream took it.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Option<StreamConfig>>>);

impl Heard {
    fn config(&self) -> Option<StreamConfig> {
        *self.0.lock().expect("no test holds this across a panic")
    }
}

impl AudioPath for Heard {
    fn prepare(&mut self, config: StreamConfig) {
        *self.0.lock().expect("no test holds this across a panic") = Some(config);
    }

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, _command: Command) -> bool {
        false
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

fn hosts_named(name: &str) -> Vec<AudioHost> {
    vec![AudioHost {
        name: name.to_owned(),
        inputs: vec![AudioDevice {
            id: DeviceId::named("in"),
            channels: vec![2],
        }],
        outputs: vec![AudioDevice {
            id: DeviceId::named("out"),
            channels: vec![2],
        }],
    }]
}

fn selection_on(name: &str) -> DeviceSelection {
    DeviceSelection {
        host: name.to_owned(),
        input: DeviceId::named("in"),
        input_channels: ChannelSelection::all(2),
        output: DeviceId::named("out"),
        output_channels: ChannelSelection::all(2),
    }
}

struct RecordingBackend(Asked);

struct RecordingStream(Asked);

impl AudioBackend for RecordingBackend {
    type Stream = RecordingStream;

    fn hosts(&self, _sample_rate: u32) -> Vec<AudioHost> {
        hosts_named("recording")
    }

    fn defaults(&self, _sample_rate: u32) -> Option<DeviceSelection> {
        Some(selection_on("recording"))
    }

    fn open<P: AudioPath>(
        &self,
        _selection: &DeviceSelection,
        _request: StreamRequest,
        _path: P,
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

/// A backend whose stream reports a level the test chose.
///
/// The stream with no hardware behind it meters nothing, so it can only ever
/// say the input is silent. This one stands in for a device with signal on it,
/// which is what says a level reaches the screen rather than merely a bar.
struct LoudBackend(Levels);

struct LoudStream(Levels);

impl AudioBackend for LoudBackend {
    type Stream = LoudStream;

    fn hosts(&self, _sample_rate: u32) -> Vec<AudioHost> {
        hosts_named("loud")
    }

    fn defaults(&self, _sample_rate: u32) -> Option<DeviceSelection> {
        Some(selection_on("loud"))
    }

    fn open<P: AudioPath>(
        &self,
        _selection: &DeviceSelection,
        _request: StreamRequest,
        _path: P,
    ) -> Result<Self::Stream, DeviceError> {
        Ok(LoudStream(self.0))
    }
}

impl DuplexStream for LoudStream {
    fn config(&self) -> StreamConfig {
        config()
    }

    fn state(&self) -> StreamState {
        StreamState::Running
    }

    fn levels(&self) -> Levels {
        self.0
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
        Ok(())
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        Ok(())
    }
}

/// A monitor over the null backend, playing what it captures.
type Monitoring<A> = Monitor<A, NullBackend, fn() -> Passthrough>;

fn hearing_a(levels: Levels) -> Monitor<Counted, LoudBackend, fn() -> Passthrough> {
    Monitor::opened(
        Counted::default(),
        LoudBackend(levels),
        request(),
        Passthrough::new,
    )
}

/// A monitor over `backend` whose streams run a path the test can read back.
fn hearing(
    heard: &Heard,
    backend: NullBackend,
) -> Monitor<Counted, NullBackend, impl FnMut() -> Heard> {
    let path = heard.clone();

    Monitor::opened(Counted::default(), backend, request(), move || path.clone())
}

fn monitoring(app: Counted, backend: NullBackend, request: StreamRequest) -> Monitoring<Counted> {
    Monitor::opened(app, backend, request, Passthrough::new)
}

fn filling() -> Monitoring<Filling> {
    Monitor::opened(
        Filling,
        NullBackend::rounding(config()),
        request(),
        Passthrough::new,
    )
}

fn playing() -> Monitoring<Counted> {
    monitoring(
        Counted::default(),
        NullBackend::rounding(config()),
        request(),
    )
}

fn unplug(monitor: &Monitoring<Counted>) {
    monitor
        .link()
        .expect("a monitor over a device that opened has a link")
        .stream()
        .expect("an open link has a stream")
        .fail(DeviceError::DeviceNotAvailable);
}

fn drawn<A: App, B: AudioBackend, F>(monitor: &mut Monitor<A, B, F>) -> Frame {
    let mut frame = Frame::blank();
    monitor.draw(frame.region());

    frame
}

fn bottom_row(frame: &Frame, columns: impl Iterator<Item = usize>) -> String {
    let row = DeviceProfile::TARGET.screen.rows - 1;

    columns
        .filter_map(|column| frame.get(column, row))
        .map(Cell::glyph)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn status(frame: &Frame) -> String {
    bottom_row(frame, 0..METER_COLUMN)
}

fn meter(frame: &Frame) -> String {
    bottom_row(frame, METER_COLUMN..DeviceProfile::TARGET.screen.columns)
}

fn run(monitor: &mut Monitoring<Counted>) -> RunReport {
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
fn a_monitor_opens_its_stream_with_the_path_it_was_given() {
    let heard = Heard::default();

    let _monitor = hearing(&heard, NullBackend::rounding(config()));

    assert_eq!(heard.config(), Some(config()));
}

#[test]
fn a_monitor_with_no_device_to_open_builds_no_path() {
    let heard = Heard::default();

    let _monitor = hearing(&heard, NullBackend::rounding(deaf()));

    assert_eq!(heard.config(), None);
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
fn a_silent_input_draws_an_empty_meter() {
    let mut monitor = playing();

    assert_eq!(
        meter(&drawn(&mut monitor)),
        format!("[{}]", "-".repeat(METER_SCALE))
    );
}

#[test]
fn the_input_level_reaches_the_screen() {
    let mut monitor = hearing_a(Levels {
        peak: 1.0,
        rms: 1.0,
    });

    assert_eq!(
        meter(&drawn(&mut monitor)),
        format!("[{}|]", "#".repeat(METER_SCALE - 1))
    );
}

#[test]
fn a_meter_leaves_the_state_beside_it_alone() {
    let mut monitor = hearing_a(Levels {
        peak: 1.0,
        rms: 1.0,
    });

    assert_eq!(status(&drawn(&mut monitor)), "audio playing");
}

#[test]
fn the_wrapped_application_still_draws() {
    let mut monitor = playing();

    assert_eq!(drawn(&mut monitor).get(0, 0), Some(Cell::new('m')));
}

#[test]
fn an_application_filling_its_region_leaves_the_status_row_alone() {
    let mut monitor = filling();

    assert_eq!(status(&drawn(&mut monitor)), "audio playing");
}

#[test]
fn an_application_filling_its_region_keeps_every_row_it_was_given() {
    let mut monitor = filling();
    let frame = drawn(&mut monitor);
    let screen = DeviceProfile::TARGET.screen;

    let filled = (0..screen.rows)
        .filter(|row| {
            (0..screen.columns).all(|column| frame.get(column, *row) == Some(Cell::new(FILL)))
        })
        .count();

    assert_eq!(filled, screen.rows - 1);
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
        Passthrough::new,
    ));

    assert_eq!(asked.of(), ["open", "start", "stop"]);
}
