//! Choosing the host, the devices and the channels from a page, against a
//! backend with no hardware behind it so that it runs where no audio device
//! exists.
//!
//! The page is driven through scripted control events, so every test states
//! what the player did to the panel rather than naming a key. What came of it
//! is read back from the selection the link is open on and from the frame the
//! page drew.

use std::sync::atomic::{AtomicBool, Ordering};

use motif::audio::{
    AudioBackend, AudioDevice, AudioHost, AudioPath, AudioState, ChannelSelection, DeviceError,
    DeviceId, DeviceLink, DeviceSelection, NullBackend, NullStream, Passthrough, SharedLink,
    StreamConfig, StreamRequest,
};
use motif::device::{Button, DeviceProfile, Encoder, ScreenProfile};
use motif::settings::{AudioPage, AudioSetting};
use motif::ui::{ControlEvent, Controls, Frame, Page, ScriptedControls, Turn};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const RATE: u32 = 48_000;
const FIRST: &str = "first";
const SECOND: &str = "second";
const THIRD: &str = "third";
const MIC: &str = "mic";
const LINE: &str = "line";
const SPARE: &str = "spare";
const GUEST: &str = "guest";
const SPEAKERS: &str = "speakers";
const PHONES: &str = "phones";
const DESK: &str = "desk";
const MONITOR: &str = "monitor";
const BOOTH: &str = "booth";
const WEDGE: &str = "wedge";

/// A backend listing two hosts, whose devices can arrive and depart between
/// enumerations, and whose spare input refuses to open.
///
/// It stands in for an interface unplugged between the moment the list was
/// drawn and the moment a row was moved, which is the case the page exists to
/// survive.
struct Studio {
    arrived: AtomicBool,
    departed: AtomicBool,
}

impl Studio {
    fn new() -> Self {
        Self {
            arrived: AtomicBool::new(false),
            departed: AtomicBool::new(false),
        }
    }

    fn arrive(&self) {
        self.arrived.store(true, Ordering::Relaxed);
    }

    fn depart(&self) {
        self.departed.store(true, Ordering::Relaxed);
    }

    fn first_inputs(&self) -> Vec<AudioDevice> {
        let mut listed = vec![device(MIC, vec![1, 2])];
        if !self.departed.load(Ordering::Relaxed) {
            listed.push(device(LINE, vec![2]));
        }
        listed.push(device(SPARE, vec![2]));
        if self.arrived.load(Ordering::Relaxed) {
            listed.push(device(GUEST, vec![2]));
        }

        listed
    }
}

fn device(name: &str, channels: Vec<u16>) -> AudioDevice {
    AudioDevice {
        id: DeviceId::named(name),
        channels,
    }
}

fn granted() -> StreamConfig {
    StreamConfig {
        sample_rate: RATE,
        block_size: 256,
        input_channels: 2,
        output_channels: 2,
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: RATE,
        block_size: 256,
    }
}

impl AudioBackend for Studio {
    type Stream = NullStream;

    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost> {
        if sample_rate != RATE {
            return Vec::new();
        }

        vec![
            AudioHost {
                name: FIRST.to_owned(),
                inputs: self.first_inputs(),
                outputs: vec![device(SPEAKERS, vec![2]), device(PHONES, vec![2])],
            },
            AudioHost {
                name: SECOND.to_owned(),
                inputs: vec![device(DESK, vec![2])],
                outputs: vec![device(MONITOR, vec![2])],
            },
            AudioHost {
                name: THIRD.to_owned(),
                inputs: vec![device(BOOTH, vec![2])],
                outputs: vec![device(WEDGE, vec![2])],
            },
        ]
    }

    fn defaults(&self, sample_rate: u32) -> Option<DeviceSelection> {
        let host = self.hosts(sample_rate).into_iter().next()?;

        Some(DeviceSelection {
            host: host.name,
            input: DeviceId::named(MIC),
            input_channels: ChannelSelection::all(2),
            output: DeviceId::named(SPEAKERS),
            output_channels: ChannelSelection::all(2),
        })
    }

    fn open<P: AudioPath>(
        &self,
        selection: &DeviceSelection,
        request: StreamRequest,
        path: P,
    ) -> Result<Self::Stream, DeviceError> {
        let host = self
            .hosts(RATE)
            .into_iter()
            .find(|host| host.name == selection.host)
            .ok_or(DeviceError::NoSuchHost)?;
        let input = host
            .inputs
            .iter()
            .find(|device| device.id == selection.input)
            .ok_or(DeviceError::NoInputDevice)?;
        if input.id.name == SPARE {
            return Err(DeviceError::DeviceNotAvailable);
        }
        host.outputs
            .iter()
            .find(|device| device.id == selection.output)
            .ok_or(DeviceError::NoOutputDevice)?;

        let null = NullBackend::rounding(granted());
        let stood_in = null.defaults(RATE).ok_or(DeviceError::DeviceNotAvailable)?;

        null.open(&stood_in, request, path)
    }
}

type StudioPage = AudioPage<Studio, fn() -> Passthrough>;

/// A page over the studio, listed and then playing, as a run composes it.
///
/// The page lists what there is to choose from and leaves the first open to
/// whatever holds the link for the run, so a test wanting a page over a running
/// device opens it here, in the order a monitor would.
fn listed_at(request: StreamRequest) -> StudioPage {
    let studio = Studio::new();
    let selection = studio
        .defaults(RATE)
        .expect("the studio has both directions");
    let mut link = SharedLink::new(DeviceLink::new(
        studio,
        request,
        selection,
        Passthrough::new as fn() -> _,
    ));

    let page = AudioPage::listing(link.clone());
    if link.change(DeviceLink::open).is_ok() {
        let _started = link.change(DeviceLink::start);
    }

    page
}

fn page() -> StudioPage {
    listed_at(request())
}

fn unlisted_page() -> StudioPage {
    listed_at(StreamRequest {
        sample_rate: 44_100,
        ..request()
    })
}

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn turned(turn: Turn) -> ControlEvent {
    ControlEvent::Turned {
        encoder: Encoder::Main,
        turn,
        shifted: false,
    }
}

fn driven_by(page: &mut StudioPage, events: impl IntoIterator<Item = ControlEvent>) {
    let mut controls = ScriptedControls::new(events);
    while let Some(event) = controls.poll() {
        page.control(event);
    }
}

fn repeated(event: ControlEvent, times: usize) -> Vec<ControlEvent> {
    std::iter::repeat_n(event, times).collect()
}

fn on(setting: AudioSetting) -> Vec<ControlEvent> {
    let row = AudioSetting::ALL
        .iter()
        .position(|listed| *listed == setting)
        .expect("every setting is in the set");

    repeated(pressed(Button::Down), row)
}

fn moved(setting: AudioSetting, events: impl IntoIterator<Item = ControlEvent>) -> StudioPage {
    let mut page = page();
    driven_by(&mut page, on(setting));
    driven_by(&mut page, events);

    page
}

fn row_of(frame: &Frame, row: usize) -> String {
    (0..SCREEN.columns)
        .filter_map(|column| frame.get(column, row))
        .filter(|cell| cell.columns() > 0)
        .map(|cell| cell.glyph())
        .collect()
}

fn drawn_into(page: &mut StudioPage, rows: usize) -> Vec<String> {
    let mut frame = Frame::blank();
    let (region, _below) = frame.region().split_top(rows);
    page.draw(region);

    (0..SCREEN.rows)
        .map(|row| row_of(&frame, row).trim_end().to_string())
        .collect()
}

fn drawn(page: &mut StudioPage) -> Vec<String> {
    drawn_into(page, SCREEN.rows)
}

fn input_of(page: &StudioPage) -> String {
    page.link().read(|held| held.selection().input.to_string())
}

fn output_of(page: &StudioPage) -> String {
    page.link().read(|held| held.selection().output.to_string())
}

fn host_of(page: &StudioPage) -> String {
    page.link().read(|held| held.selection().host.clone())
}

fn input_channels_of(page: &StudioPage) -> ChannelSelection {
    page.link().read(|held| held.selection().input_channels)
}

fn output_channels_of(page: &StudioPage) -> ChannelSelection {
    page.link().read(|held| held.selection().output_channels)
}

#[test]
fn a_new_page_selects_the_host() {
    assert_eq!(page().selected(), AudioSetting::Host);
}

#[test]
fn an_opened_page_is_playing_what_it_was_given() {
    let page = page();

    assert_eq!(page.state(), AudioState::Playing);
    assert_eq!(host_of(&page), FIRST);
    assert_eq!(input_of(&page), MIC);
}

#[test]
fn an_opened_page_has_listed_what_there_is_to_choose_from() {
    let page = page();

    let named: Vec<&str> = page
        .listed()
        .iter()
        .map(|host| host.name.as_str())
        .collect();

    assert_eq!(named, [FIRST, SECOND, THIRD]);
}

#[test]
fn down_moves_to_the_next_setting() {
    let mut page = page();
    driven_by(&mut page, [pressed(Button::Down)]);

    assert_eq!(page.selected(), AudioSetting::Input);
}

#[test]
fn up_moves_back_to_the_setting_before() {
    let mut page = page();
    driven_by(
        &mut page,
        [
            pressed(Button::Down),
            pressed(Button::Down),
            pressed(Button::Up),
        ],
    );

    assert_eq!(page.selected(), AudioSetting::Input);
}

#[test]
fn up_at_the_first_setting_stays_there() {
    let mut page = page();
    driven_by(&mut page, repeated(pressed(Button::Up), 3));

    assert_eq!(page.selected(), AudioSetting::Host);
}

#[test]
fn down_at_the_last_setting_stays_there() {
    let mut page = page();
    driven_by(&mut page, repeated(pressed(Button::Down), 9));

    assert_eq!(page.selected(), AudioSetting::OutputChannels);
}

#[test]
fn every_setting_is_drawn_with_its_value() {
    let mut page = page();

    let rows = drawn(&mut page);

    assert!(rows[0].starts_with("> host"), "{}", rows[0]);
    assert!(rows[0].ends_with(FIRST), "{}", rows[0]);
    assert!(rows[1].ends_with(MIC), "{}", rows[1]);
    assert!(rows[2].starts_with("  input channels"), "{}", rows[2]);
    assert!(rows[3].ends_with(SPEAKERS), "{}", rows[3]);
    assert!(rows[4].starts_with("  output channels"), "{}", rows[4]);
}

#[test]
fn only_the_selected_setting_is_marked() {
    let mut page = page();
    driven_by(&mut page, [pressed(Button::Down)]);

    let marked: Vec<String> = drawn(&mut page)
        .into_iter()
        .filter(|row| row.starts_with('>'))
        .collect();

    assert_eq!(marked.len(), 1);
    assert!(marked[0].starts_with("> input"), "{}", marked[0]);
}

#[test]
fn moving_the_host_opens_the_next_one() {
    let page = moved(AudioSetting::Host, [pressed(Button::Right)]);

    assert_eq!(host_of(&page), SECOND);
    assert_eq!(page.state(), AudioState::Playing);
}

#[test]
fn a_new_host_brings_its_own_devices() {
    let page = moved(AudioSetting::Host, [pressed(Button::Right)]);

    assert_eq!(input_of(&page), DESK);
    assert_eq!(output_of(&page), MONITOR);
}

#[test]
fn moving_the_host_back_returns_to_the_first() {
    let page = moved(
        AudioSetting::Host,
        [pressed(Button::Right), pressed(Button::Left)],
    );

    assert_eq!(host_of(&page), FIRST);
    assert_eq!(input_of(&page), MIC);
}

#[test]
fn the_host_steps_on_from_the_one_that_is_open() {
    let page = moved(AudioSetting::Host, repeated(pressed(Button::Right), 2));

    assert_eq!(host_of(&page), THIRD);
    assert_eq!(input_of(&page), BOOTH);
}

#[test]
fn the_host_stops_at_the_last_one_listed() {
    let page = moved(AudioSetting::Host, repeated(pressed(Button::Right), 4));

    assert_eq!(host_of(&page), THIRD);
}

#[test]
fn moving_the_input_opens_the_next_device() {
    let page = moved(AudioSetting::Input, [pressed(Button::Right)]);

    assert_eq!(input_of(&page), LINE);
    assert_eq!(page.state(), AudioState::Playing);
}

#[test]
fn turning_the_encoder_moves_a_value_like_the_arrows() {
    let clockwise = moved(AudioSetting::Input, [turned(Turn::Clockwise)]);
    let back = moved(
        AudioSetting::Input,
        [turned(Turn::Clockwise), turned(Turn::Anticlockwise)],
    );

    assert_eq!(input_of(&clockwise), LINE);
    assert_eq!(input_of(&back), MIC);
}

#[test]
fn moving_the_output_opens_the_next_device() {
    let page = moved(AudioSetting::Output, [pressed(Button::Right)]);

    assert_eq!(output_of(&page), PHONES);
    assert_eq!(page.state(), AudioState::Playing);
}

#[test]
fn a_new_input_device_is_taken_whole() {
    let mut page = page();
    driven_by(&mut page, on(AudioSetting::InputChannels));
    driven_by(&mut page, [pressed(Button::Left)]);
    driven_by(&mut page, [pressed(Button::Up), pressed(Button::Right)]);

    assert_eq!(input_channels_of(&page), ChannelSelection::all(2));
}

#[test]
fn a_new_output_device_is_taken_whole() {
    let mut page = page();
    driven_by(&mut page, on(AudioSetting::OutputChannels));
    driven_by(&mut page, [pressed(Button::Left)]);
    driven_by(&mut page, [pressed(Button::Up), pressed(Button::Right)]);

    assert_eq!(output_channels_of(&page), ChannelSelection::all(2));
}

#[test]
fn moving_the_input_channels_narrows_what_is_captured() {
    let page = moved(AudioSetting::InputChannels, [pressed(Button::Left)]);

    assert_eq!(
        input_channels_of(&page),
        ChannelSelection { first: 1, count: 1 }
    );
}

#[test]
fn the_input_channels_stop_at_the_whole_device() {
    let page = moved(
        AudioSetting::InputChannels,
        repeated(pressed(Button::Right), 3),
    );

    assert_eq!(input_channels_of(&page), ChannelSelection::all(2));
}

#[test]
fn moving_the_output_channels_narrows_what_is_played() {
    let page = moved(AudioSetting::OutputChannels, [pressed(Button::Left)]);

    assert_eq!(
        output_channels_of(&page),
        ChannelSelection { first: 1, count: 1 }
    );
}

#[test]
fn a_channel_run_is_named_by_the_channels_a_player_counts() {
    let whole = page();
    let second = moved(AudioSetting::InputChannels, [pressed(Button::Left)]);
    let first = moved(
        AudioSetting::InputChannels,
        repeated(pressed(Button::Left), 2),
    );

    assert_eq!(whole.value(AudioSetting::InputChannels), "1-2");
    assert_eq!(second.value(AudioSetting::InputChannels), "2");
    assert_eq!(first.value(AudioSetting::InputChannels), "1");
}

#[test]
fn a_device_that_cannot_be_opened_leaves_the_previous_one_running() {
    let page = moved(AudioSetting::Input, repeated(pressed(Button::Right), 2));

    assert_eq!(input_of(&page), LINE);
    assert_eq!(page.state(), AudioState::Playing);
}

#[test]
fn a_choice_that_cannot_be_opened_says_why() {
    let mut page = moved(AudioSetting::Input, repeated(pressed(Button::Right), 2));

    assert_eq!(page.refused(), Some(DeviceError::DeviceNotAvailable));
    assert!(
        drawn(&mut page)
            .iter()
            .any(|row| row.contains(&DeviceError::DeviceNotAvailable.to_string())),
    );
}

#[test]
fn the_reason_is_drawn_under_the_settings() {
    let mut page = moved(AudioSetting::Input, repeated(pressed(Button::Right), 2));
    let last = AudioSetting::ALL.len();

    let rows = drawn(&mut page);

    assert_eq!(rows[last], "");
    assert!(rows[last + 1].contains("cannot open"), "{}", rows[last + 1]);
}

#[test]
fn a_page_given_fewer_rows_draws_nothing_below_them() {
    let mut page = moved(AudioSetting::Input, repeated(pressed(Button::Right), 2));
    let band = 2;

    let rows = drawn_into(&mut page, band);

    assert!(
        rows[band..].iter().all(String::is_empty),
        "{:?}",
        &rows[band..],
    );
}

#[test]
fn a_choice_that_opens_clears_the_refusal() {
    let mut page = moved(AudioSetting::Input, repeated(pressed(Button::Right), 2));
    driven_by(&mut page, [pressed(Button::Left)]);

    assert_eq!(page.refused(), None);
    assert_eq!(input_of(&page), MIC);
}

#[test]
fn a_page_with_nothing_listed_still_draws_what_is_open() {
    let mut page = unlisted_page();

    assert!(page.listed().is_empty());
    assert_eq!(page.value(AudioSetting::Host), FIRST);
    assert!(drawn(&mut page)[1].ends_with(MIC));
}

#[test]
fn a_setting_with_nothing_listed_does_not_move() {
    let mut page = unlisted_page();
    driven_by(&mut page, [pressed(Button::Right)]);

    assert_eq!(host_of(&page), FIRST);
    assert_eq!(page.state(), AudioState::Playing);
}

#[test]
fn refreshing_lists_a_device_that_arrived() {
    let mut page = page();
    page.link().read(|held| held.backend().arrive());

    page.refresh();

    assert!(
        page.listed()[0]
            .inputs
            .iter()
            .any(|device| device.id.name == GUEST)
    );
}

#[test]
fn refreshing_keeps_the_device_that_is_open() {
    let mut page = moved(AudioSetting::Input, [pressed(Button::Right)]);
    page.link().read(|held| held.backend().depart());

    page.refresh();

    assert!(
        page.listed()[0]
            .inputs
            .iter()
            .any(|device| device.id.name == LINE)
    );
}
