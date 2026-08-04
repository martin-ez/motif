//! The looper screen: what a press does to the transport, and what the player
//! is shown while it runs.
//!
//! Nothing here names a key or a terminal. The page is driven with the panel's
//! controls and read back off the frame it drew, which is the whole of what an
//! application is allowed to know about either end.

use motif::device::{Button, DeviceProfile, Encoder, ScreenProfile};
use motif::looper::{LoopPosition, LooperPage, Transport, position_meter};
use motif::ui::{App, ControlEvent, Flow, Frame, Turn};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const SECOND: u32 = DeviceProfile::TARGET.audio.sample_rate;

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn page() -> LooperPage {
    LooperPage::new(position_meter().1)
}

fn page_showing(position: LoopPosition) -> LooperPage {
    let (mut writer, reader) = position_meter();
    writer.publish(position);

    LooperPage::new(reader)
}

fn driven_by(buttons: impl IntoIterator<Item = Button>) -> LooperPage {
    let mut page = page();
    for button in buttons {
        page.control(pressed(button));
    }

    page
}

fn row_of(frame: &Frame, row: usize) -> String {
    (0..SCREEN.columns)
        .filter_map(|column| frame.get(column, row))
        .map(|cell| cell.glyph())
        .collect()
}

fn drawn(page: &mut LooperPage) -> Vec<String> {
    let mut frame = Frame::blank();
    page.draw(&mut frame);

    (0..SCREEN.rows).map(|row| row_of(&frame, row)).collect()
}

fn screen_of(page: &mut LooperPage) -> String {
    drawn(page).join("\n")
}

struct Bar {
    filled: usize,
    width: usize,
}

fn bar_of(page: &mut LooperPage) -> Bar {
    let drawn = drawn(page);
    let bar = drawn
        .iter()
        .map(|row| row.trim_end())
        .find(|row| row.starts_with('['))
        .expect("the page draws a loop bar");

    assert!(
        bar.ends_with(']'),
        "a bar that runs off the edge of the screen is not a bar: {bar:?}"
    );

    Bar {
        filled: bar.chars().filter(|glyph| *glyph == '#').count(),
        width: bar.chars().count() - 2,
    }
}

#[test]
fn a_page_starts_with_nothing_recorded() {
    assert_eq!(page().transport(), Transport::Idle);
}

#[test]
fn record_starts_the_first_take() {
    assert_eq!(
        driven_by([Button::Record]).transport(),
        Transport::Recording
    );
}

#[test]
fn record_again_layers_onto_the_take() {
    assert_eq!(
        driven_by([Button::Record, Button::Record]).transport(),
        Transport::Overdubbing
    );
}

#[test]
fn play_closes_the_take_and_plays_it() {
    assert_eq!(
        driven_by([Button::Record, Button::Play]).transport(),
        Transport::Playing
    );
}

#[test]
fn stop_closes_the_take_and_halts() {
    assert_eq!(
        driven_by([Button::Record, Button::Stop]).transport(),
        Transport::Stopped
    );
}

#[test]
fn a_navigation_button_leaves_the_transport_alone() {
    assert_eq!(
        driven_by([Button::Record, Button::Up, Button::Left]).transport(),
        Transport::Recording
    );
}

#[test]
fn an_encoder_leaves_the_transport_alone() {
    let mut page = driven_by([Button::Record]);

    page.control(ControlEvent::Turned {
        encoder: Encoder::First,
        turn: Turn::Clockwise,
        shifted: false,
    });

    assert_eq!(page.transport(), Transport::Recording);
}

#[test]
fn a_control_never_ends_the_run() {
    let mut page = page();

    for button in Button::ALL {
        assert_eq!(page.control(pressed(button)), Flow::Continue);
    }
}

#[test]
fn a_draw_never_ends_the_run() {
    assert_eq!(page().draw(&mut Frame::blank()), Flow::Continue);
}

#[test]
fn an_idle_page_says_so() {
    assert!(screen_of(&mut page()).contains("IDLE"));
}

#[test]
fn the_transport_state_is_on_screen() {
    assert!(screen_of(&mut driven_by([Button::Record])).contains("RECORDING"));
}

#[test]
fn an_overdub_is_named_on_screen() {
    let mut layering = driven_by([Button::Record, Button::Record]);

    assert!(screen_of(&mut layering).contains("OVERDUBBING"));
}

#[test]
fn a_capturing_transport_is_marked_armed() {
    assert!(screen_of(&mut driven_by([Button::Record])).contains("ARMED"));
}

#[test]
fn a_transport_that_captures_nothing_is_not_marked_armed() {
    let mut playing = driven_by([Button::Record, Button::Play]);

    assert!(!screen_of(&mut playing).contains("ARMED"));
}

#[test]
fn the_readout_shows_the_published_position() {
    let mut page = page_showing(LoopPosition::new(3 * SECOND / 2, 8 * SECOND));

    assert!(screen_of(&mut page).contains("0:01.5 / 0:08.0"));
}

#[test]
fn a_loop_past_a_minute_reads_in_minutes() {
    let mut page = page_showing(LoopPosition::new(75 * SECOND, 90 * SECOND));

    assert!(screen_of(&mut page).contains("1:15.0 / 1:30.0"));
}

#[test]
fn an_empty_loop_reads_as_nothing_recorded() {
    assert!(screen_of(&mut page()).contains("0:00.0 / 0:00.0"));
}

#[test]
fn the_bar_fills_with_the_playhead() {
    let quarter = bar_of(&mut page_showing(LoopPosition::new(2 * SECOND, 8 * SECOND)));

    assert_eq!(quarter.filled * 4, quarter.width);
}

#[test]
fn a_finished_loop_fills_the_bar() {
    let bar = bar_of(&mut page_showing(LoopPosition::new(8 * SECOND, 8 * SECOND)));

    assert_eq!(bar.filled, bar.width);
}

#[test]
fn an_empty_loop_draws_an_empty_bar() {
    assert_eq!(bar_of(&mut page()).filled, 0);
}

#[test]
fn the_bar_spans_the_screen() {
    assert_eq!(bar_of(&mut page()).width, SCREEN.columns - 2);
}

#[test]
fn the_page_declares_what_the_transport_buttons_do() {
    let legend = page().legend();

    assert_eq!(legend.meaning(Button::Play), Some("play"));
    assert_eq!(legend.meaning(Button::Stop), Some("stop"));
    assert_eq!(legend.meaning(Button::Record), Some("rec"));
}

#[test]
fn the_page_declares_nothing_for_a_control_it_leaves_alone() {
    let legend = page().legend();

    assert_eq!(legend.meaning(Button::Up), None);
    assert_eq!(legend.meaning(Encoder::First), None);
}
