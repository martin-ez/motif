//! Composing the application a run is: the looper page and the settings page
//! under one shell, over one device, against a backend that counts the streams
//! it has open.
//!
//! Both halves reach the device through a link, and a run has one device: what
//! these say is that composing them opens one stream and not two, that a choice
//! made on the page still re-opens it, and that each page is reached from the
//! other without a control meant for one landing on the other.
//!
//! The pages are the real ones rather than stand-ins: a page contract that is
//! enough for one screen and not for two is only visible against two.
//!
//! The backend counts rather than records, because two streams open at once is
//! not visible in an order of calls — each one opens and starts exactly as a
//! single stream would.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use motif::audio::{
    AudioBackend, AudioDevice, AudioHost, AudioPath, AudioState, ChannelSelection, DeviceError,
    DeviceId, DeviceSelection, DuplexStream, Headroom, Levels, Passthrough, Placed, SharedLink,
    Slack, StreamConfig, StreamRequest, StreamState, Xruns, sample_clock,
};
use motif::device::{Button, DeviceProfile};
use motif::looper::{LooperPage, marks_handoff};
use motif::monitor::Monitor;
use motif::settings::{AudioPage, AudioSetting};
use motif::ui::{App, Cell as Glyph, ControlEvent, Frame, Scheme, Shell};

const HOST: &str = "counted";
const FIRST_INPUT: &str = "first input";
const SECOND_INPUT: &str = "second input";
const OUTPUT: &str = "output";
const CHANNELS: u16 = 2;
const IDLE: &str = "IDLE";
const SETTLING_FRAMES: usize = 3;
const ANSWERING_FRAMES: usize = 2_000;

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 256,
        input_channels: CHANNELS,
        output_channels: CHANNELS,
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: 48_000,
        block_size: 256,
    }
}

fn device(name: &str) -> AudioDevice {
    AudioDevice {
        id: DeviceId::named(name),
        channels: vec![CHANNELS],
    }
}

/// How many streams are open and how many have ever been, shared by the backend
/// and every stream it opened.
///
/// Two counts rather than one, because a replacement is invisible in the first:
/// a stream that closes as another opens leaves one open either way, and what
/// says a device was not replaced is that none was ever built.
#[derive(Clone, Default)]
struct Live {
    open: Arc<AtomicUsize>,
    ever: Arc<AtomicUsize>,
}

impl Live {
    fn count(&self) -> usize {
        self.open.load(Ordering::Relaxed)
    }

    fn ever(&self) -> usize {
        self.ever.load(Ordering::Relaxed)
    }

    fn opened(&self) {
        self.open.fetch_add(1, Ordering::Relaxed);
        self.ever.fetch_add(1, Ordering::Relaxed);
    }

    fn closed(&self) {
        self.open.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A backend with two input devices, counting the streams it has open.
struct CountingBackend(Live);

struct CountingStream(Live);

impl AudioBackend for CountingBackend {
    type Stream = CountingStream;

    fn hosts(&self, _sample_rate: u32) -> Vec<AudioHost> {
        vec![AudioHost {
            name: HOST.to_owned(),
            inputs: vec![device(FIRST_INPUT), device(SECOND_INPUT)],
            outputs: vec![device(OUTPUT)],
        }]
    }

    fn defaults(&self, _sample_rate: u32) -> Option<DeviceSelection> {
        Some(DeviceSelection {
            host: HOST.to_owned(),
            input: DeviceId::named(FIRST_INPUT),
            input_channels: ChannelSelection::all(CHANNELS),
            output: DeviceId::named(OUTPUT),
            output_channels: ChannelSelection::all(CHANNELS),
        })
    }

    fn open<P: AudioPath>(
        &self,
        selection: &DeviceSelection,
        _request: StreamRequest,
        _path: P,
    ) -> Result<Self::Stream, DeviceError> {
        if selection.host != HOST {
            return Err(DeviceError::NoSuchHost);
        }
        self.0.opened();

        Ok(CountingStream(self.0.clone()))
    }
}

impl Drop for CountingStream {
    fn drop(&mut self) {
        self.0.closed();
    }
}

impl DuplexStream for CountingStream {
    fn config(&self) -> StreamConfig {
        config()
    }

    fn state(&self) -> StreamState {
        StreamState::Running
    }

    fn captured(&self) -> Levels {
        Levels::SILENT
    }

    fn played(&self) -> Levels {
        Levels::SILENT
    }

    fn xruns(&self) -> Xruns {
        Xruns::NONE
    }

    fn slack(&self) -> Slack {
        Slack::NONE
    }

    fn headroom(&self) -> Headroom {
        Headroom::IDLE
    }

    fn placement(&self) -> Placed {
        Placed::UNASKED
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

/// The pages, the shell and the monitor over one link, as a run composes them.
type Composed = Monitor<Shell, CountingBackend, fn() -> Passthrough>;

fn composed(live: &Live) -> Composed {
    let audio = DeviceProfile::TARGET.audio;
    let link = SharedLink::defaulting(
        CountingBackend(live.clone()),
        request(),
        Passthrough::new as fn() -> Passthrough,
    )
    .expect("the counting backend has a device in each direction");
    let settings = AudioPage::listing(link.clone());
    let (looper, _engine, _takes) =
        LooperPage::driving(audio, marks_handoff().1, sample_clock(audio.sample_rate).1);
    let shell = Shell::navigated_by([Box::new(looper), Box::new(settings)], Scheme::scenes());

    Monitor::watching(shell, Some(link))
}

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn onto_the_settings(monitor: &mut Composed) {
    monitor.control(pressed(Button::SecondScene));
}

fn onto_the_looper(monitor: &mut Composed) {
    monitor.control(pressed(Button::FirstScene));
}

fn onto_the_second_input(monitor: &mut Composed) {
    choosing_the_second_input(monitor);
    settled(monitor);
}

fn choosing_the_second_input(monitor: &mut Composed) {
    onto_the_settings(monitor);
    monitor.control(pressed(Button::Down));
    monitor.control(pressed(Button::Right));
}

fn chosen(monitor: &mut Composed) -> Frame {
    let mut frame = Frame::blank();
    for _ in 0..SETTLING_FRAMES {
        frame = drawn(monitor);
    }

    frame
}

fn settled(monitor: &mut Composed) {
    let _chosen = chosen(monitor);
    let _dispatched = drawn(monitor);
    answered(monitor);
}

fn answered(monitor: &mut Composed) {
    for _ in 0..ANSWERING_FRAMES {
        if monitor.state() != AudioState::Opening {
            return;
        }
        let _waiting = drawn(monitor);
    }

    panic!("the device never answered");
}

fn drawn(monitor: &mut Composed) -> Frame {
    let mut frame = Frame::blank();
    monitor.draw(frame.region());

    frame
}

fn row(frame: &Frame, row: usize) -> String {
    (0..DeviceProfile::TARGET.screen.columns)
        .filter_map(|column| frame.get(column, row))
        .map(Glyph::glyph)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn status(frame: &Frame) -> String {
    row(frame, DeviceProfile::TARGET.screen.rows - 1)
}

fn input_of(monitor: &Composed) -> String {
    monitor
        .link()
        .expect("a monitor over a device that opened has a link")
        .read(|held| held.selection().input.to_string())
}

#[test]
fn composing_the_page_and_the_monitor_opens_one_stream() {
    let live = Live::default();
    let _composed = composed(&live);

    assert_eq!(live.count(), 1);
}

#[test]
fn a_choice_on_the_page_re_opens_the_one_stream() {
    let live = Live::default();
    let mut monitor = composed(&live);

    onto_the_second_input(&mut monitor);

    assert_eq!(live.count(), 1);
}

#[test]
fn the_monitor_holds_the_stream_the_page_chose() {
    let live = Live::default();
    let mut monitor = composed(&live);

    onto_the_second_input(&mut monitor);

    assert_eq!(input_of(&monitor), SECOND_INPUT);
}

#[test]
fn the_status_row_draws_what_the_page_is_showing() {
    let live = Live::default();
    let mut monitor = composed(&live);

    onto_the_second_input(&mut monitor);
    let frame = drawn(&mut monitor);

    assert!(status(&frame).starts_with("audio playing"));
    assert!(row(&frame, AudioSetting::Input as usize).ends_with(SECOND_INPUT));
}

#[test]
fn the_status_row_says_the_device_is_opening() {
    let live = Live::default();
    let mut monitor = composed(&live);

    choosing_the_second_input(&mut monitor);
    let frame = chosen(&mut monitor);

    assert!(
        status(&frame).starts_with("audio opening"),
        "{}",
        status(&frame)
    );
}

#[test]
fn the_stream_that_is_running_is_the_only_one_while_the_next_is_chosen() {
    let live = Live::default();
    let mut monitor = composed(&live);

    choosing_the_second_input(&mut monitor);
    let _frame = chosen(&mut monitor);

    assert_eq!(live.ever(), 1);
    assert_eq!(live.count(), 1);
}

#[test]
fn a_composed_run_opens_on_the_looper() {
    let live = Live::default();
    let mut monitor = composed(&live);

    let frame = drawn(&mut monitor);

    assert!(row(&frame, 0).starts_with(IDLE));
}

#[test]
fn a_scene_reaches_the_settings_from_the_looper() {
    let live = Live::default();
    let mut monitor = composed(&live);

    onto_the_settings(&mut monitor);
    let frame = drawn(&mut monitor);

    assert!(row(&frame, AudioSetting::Input as usize).ends_with(FIRST_INPUT));
}

#[test]
fn a_scene_reaches_the_looper_from_the_settings() {
    let live = Live::default();
    let mut monitor = composed(&live);

    onto_the_settings(&mut monitor);
    onto_the_looper(&mut monitor);
    let frame = drawn(&mut monitor);

    assert!(row(&frame, 0).starts_with(IDLE));
}

#[test]
fn a_control_the_looper_answers_does_not_reach_it_from_the_settings() {
    let live = Live::default();
    let mut monitor = composed(&live);

    onto_the_settings(&mut monitor);
    monitor.control(pressed(Button::Record));
    onto_the_looper(&mut monitor);
    let frame = drawn(&mut monitor);

    assert!(row(&frame, 0).starts_with(IDLE));
}

#[test]
fn a_control_the_settings_answer_does_not_reach_them_from_the_looper() {
    let live = Live::default();
    let mut monitor = composed(&live);

    monitor.control(pressed(Button::Down));
    monitor.control(pressed(Button::Right));
    onto_the_settings(&mut monitor);
    let frame = drawn(&mut monitor);

    assert!(row(&frame, AudioSetting::Input as usize).ends_with(FIRST_INPUT));
}

#[test]
fn a_composed_run_leaves_no_stream_open() {
    let live = Live::default();

    drop(composed(&live));

    assert_eq!(live.count(), 0);
}

#[test]
fn a_composed_run_plays() {
    let live = Live::default();

    assert_eq!(composed(&live).state(), AudioState::Playing);
}
