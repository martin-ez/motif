//! The looper screen: what a press does to the transport, and what the player
//! is shown while it runs.
//!
//! Nothing here names a key or a terminal. The page is driven with the panel's
//! controls and read back off the frame it drew, which is the whole of what an
//! application is allowed to know about either end.

use motif::audio::{SampleClockWriter, sample_clock};
use motif::device::{Button, DeviceProfile, Encoder, ScreenProfile};
use motif::looper::{LoopPosition, LooperPage, Transport, position_meter};
use motif::seq::TapTempo;
use motif::ui::{ControlEvent, Frame, Page, Turn};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const SECOND: u32 = DeviceProfile::TARGET.audio.sample_rate;

/// Half a second of frames, which is 120 BPM.
const HALF_SECOND: usize = SECOND as usize / 2;

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn shifted(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: true,
    }
}

fn page() -> LooperPage {
    LooperPage::new(position_meter().1, sample_clock(SECOND).1)
}

fn page_showing(position: LoopPosition) -> LooperPage {
    let (mut writer, reader) = position_meter();
    writer.publish(position);

    LooperPage::new(reader, sample_clock(SECOND).1)
}

fn page_on_a_clock_at(sample_rate: u32) -> (LooperPage, SampleClockWriter) {
    let (frames, elapsed) = sample_clock(sample_rate);

    (LooperPage::new(position_meter().1, elapsed), frames)
}

fn page_on_a_clock() -> (LooperPage, SampleClockWriter) {
    page_on_a_clock_at(SECOND)
}

fn tapped(apart: &[usize]) -> LooperPage {
    let (mut page, mut frames) = page_on_a_clock();

    page.control(shifted(Button::Play));
    for interval in apart {
        frames.advance(*interval);
        page.control(shifted(Button::Play));
    }

    page
}

fn tapped_steadily() -> LooperPage {
    tapped(&[HALF_SECOND; TapTempo::TAPS_TO_A_TEMPO - 1])
}

fn turned(turn: Turn) -> ControlEvent {
    ControlEvent::Turned {
        encoder: Encoder::Main,
        turn,
        shifted: false,
    }
}

fn turned_repeatedly(times: usize, turn: Turn) -> LooperPage {
    let mut page = page();
    for _ in 0..times {
        page.control(turned(turn));
    }

    page
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
    page.draw(frame.region());

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
        encoder: Encoder::Main,
        turn: Turn::Clockwise,
        shifted: false,
    });

    assert_eq!(page.transport(), Transport::Recording);
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
fn a_page_starts_with_nothing_tapped() {
    let mut page = page();

    assert!(page.grid().is_empty());
    assert!(!screen_of(&mut page).contains("BPM"));
}

#[test]
fn a_tap_lands_on_the_grid_where_the_clock_had_got_to() {
    let page = tapped_steadily();

    assert_eq!(
        page.grid().beats(),
        &[0, HALF_SECOND as u64, 2 * HALF_SECOND as u64]
    );
}

#[test]
fn taps_are_read_at_the_rate_the_clock_counts_at() {
    let half_rate = SECOND / 2;
    let (mut page, mut frames) = page_on_a_clock_at(half_rate);
    for _ in 0..TapTempo::TAPS_TO_A_TEMPO {
        page.control(shifted(Button::Play));
        frames.advance(half_rate as usize / 2);
    }

    assert_eq!(page.grid().sample_rate(), half_rate);
    assert!(screen_of(&mut page).contains("120.0 BPM"));
}

#[test]
fn a_tapped_pulse_puts_its_tempo_on_the_screen() {
    assert!(screen_of(&mut tapped_steadily()).contains("120.0 BPM"));
}

#[test]
fn a_slower_pulse_reads_as_a_slower_tempo() {
    let mut slower = tapped(&[SECOND as usize, SECOND as usize]);

    assert!(screen_of(&mut slower).contains("60.0 BPM"));
}

#[test]
fn a_pulse_nobody_has_stated_yet_shows_no_tempo() {
    let mut page = tapped(&[HALF_SECOND]);

    assert!(!screen_of(&mut page).contains("BPM"));
}

#[test]
fn a_tap_after_a_long_silence_takes_the_tempo_off_the_screen() {
    let (mut page, mut frames) = page_on_a_clock();
    for _ in 0..TapTempo::TAPS_TO_A_TEMPO {
        frames.advance(HALF_SECOND);
        page.control(shifted(Button::Play));
    }

    frames.advance(SECOND as usize * TapTempo::STALE_AFTER_SECONDS as usize + 1);
    page.control(shifted(Button::Play));

    assert!(!screen_of(&mut page).contains("BPM"));
}

#[test]
fn tapping_leaves_the_transport_alone() {
    let (mut page, _frames) = page_on_a_clock();
    page.control(pressed(Button::Record));

    page.control(shifted(Button::Play));

    assert_eq!(page.transport(), Transport::Recording);
}

#[test]
fn play_without_shift_still_plays_rather_than_tapping() {
    let page = driven_by([Button::Record, Button::Play]);

    assert_eq!(page.transport(), Transport::Playing);
    assert!(page.grid().is_empty());
}

#[test]
fn the_page_declares_the_transport_buttons_it_answers() {
    let legend = page().legend();

    assert!(legend.answers(Button::Play));
    assert!(legend.answers(Button::Stop));
    assert!(legend.answers(Button::Record));
}

#[test]
fn the_page_declares_nothing_for_a_control_it_leaves_alone() {
    let legend = page().legend();

    assert!(!legend.answers(Button::Up));
    assert!(!legend.answers(Button::FirstScene));
}

#[test]
fn the_page_declares_the_encoder_the_gain_moves_by() {
    assert!(page().legend().answers(Encoder::Main));
}

#[test]
fn a_new_page_is_at_unity_and_unmuted() {
    let page = page();

    assert_eq!(page.gain(), 1.0);
    assert_eq!(page.decibels(), 0.0);
    assert!(!page.muted());
}

#[test]
fn turning_the_encoder_clockwise_raises_the_gain() {
    let page = turned_repeatedly(3, Turn::Clockwise);

    assert_eq!(page.decibels(), 3.0);
    assert!(page.gain() > 1.0);
}

#[test]
fn turning_the_encoder_anticlockwise_lowers_the_gain() {
    let page = turned_repeatedly(6, Turn::Anticlockwise);

    assert_eq!(page.decibels(), -6.0);
    assert!(page.gain() < 1.0);
}

#[test]
fn six_decibels_down_is_about_half_the_level() {
    let page = turned_repeatedly(6, Turn::Anticlockwise);

    assert!((page.gain() - 0.5).abs() < 0.01);
}

#[test]
fn the_gain_stops_at_the_top_of_its_range() {
    let mut page = turned_repeatedly(200, Turn::Clockwise);
    let ceiling = page.decibels();

    page.control(turned(Turn::Clockwise));

    assert_eq!(page.decibels(), ceiling);
    assert!(ceiling > 0.0);
}

#[test]
fn the_gain_stops_at_the_bottom_of_its_range() {
    let mut page = turned_repeatedly(200, Turn::Anticlockwise);
    let floor = page.decibels();

    page.control(turned(Turn::Anticlockwise));

    assert_eq!(page.decibels(), floor);
    assert!(page.gain() < 0.01);
}

#[test]
fn shift_and_record_mutes_the_input() {
    let mut page = page();

    page.control(shifted(Button::Record));

    assert!(page.muted());
}

#[test]
fn shift_and_record_again_unmutes_it() {
    let mut page = page();

    page.control(shifted(Button::Record));
    page.control(shifted(Button::Record));

    assert!(!page.muted());
}

#[test]
fn muting_leaves_the_transport_where_it_was() {
    let mut page = page();

    page.control(shifted(Button::Record));

    assert_eq!(page.transport(), Transport::Idle);
}

#[test]
fn record_on_its_own_still_drives_the_transport() {
    let mut page = page();

    page.control(pressed(Button::Record));

    assert_eq!(page.transport(), Transport::Recording);
    assert!(!page.muted());
}

#[test]
fn muting_keeps_the_gain_the_player_set() {
    let mut page = turned_repeatedly(3, Turn::Anticlockwise);

    page.control(shifted(Button::Record));

    assert_eq!(page.decibels(), -3.0);
}

#[test]
fn the_gain_is_drawn_where_the_player_can_see_it() {
    let mut page = turned_repeatedly(3, Turn::Anticlockwise);

    assert!(screen_of(&mut page).contains("-3.0 dB"));
}

#[test]
fn a_muted_input_says_so_on_screen() {
    let mut page = page();
    page.control(shifted(Button::Record));

    assert!(screen_of(&mut page).contains("MUTE"));
}

#[test]
fn an_unmuted_input_says_nothing_about_muting() {
    assert!(!screen_of(&mut page()).contains("MUTE"));
}

#[test]
fn turning_the_encoder_leaves_the_transport_alone() {
    let mut page = driven_by([Button::Record]);

    page.control(turned(Turn::Clockwise));

    assert_eq!(page.transport(), Transport::Recording);
}
