//! The looper screen: what a press does to the transport, and what the player
//! is shown while it runs.
//!
//! Nothing here names a key or a terminal. The page is driven with the panel's
//! controls and read back off the frame it drew, which is the whole of what an
//! application is allowed to know about either end.

use motif::audio::{
    Command, CommandReceiver, CommandSender, SampleClockWriter, command_channel, sample_clock,
};
use motif::device::{Button, DeviceProfile, Encoder, ScreenProfile};
use motif::looper::{
    LoopBuffer, LoopPosition, LoopWaveform, LooperPage, Transport, position_meter, waveform_meter,
};
use motif::seq::TapTempo;
use motif::ui::{ControlEvent, Frame, Page, Turn};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const SECOND: u32 = DeviceProfile::TARGET.audio.sample_rate;

/// Half a second of frames, which is 120 BPM.
const HALF_SECOND: usize = SECOND as usize / 2;

/// The stack a loop with something on it is at least as deep as, so a position
/// drawn for its playhead is not also asserting an empty stack.
const A_TAKE: usize = 1;

/// Room for more commands than any test sends, so a refused send is a fact
/// about the page rather than about the queue.
const ROOM: usize = 8;

/// More detents than the gain has decibels to climb, so the turns past the top
/// of the range are turns the page has already refused.
const TURNS_PAST_THE_CEILING: usize = 24;

/// Room for a command per detent, so a gain that goes unordered is the page
/// declining to send it rather than the queue refusing to take it.
const ROOM_FOR_THE_WHOLE_RANGE: usize = TURNS_PAST_THE_CEILING;

fn sending() -> CommandSender {
    command_channel(ROOM).0
}

fn ordered(orders: &mut CommandReceiver) -> Vec<Command> {
    orders.drain().collect()
}

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
    LooperPage::new(
        position_meter().1,
        waveform_meter().1,
        sample_clock(SECOND).1,
        sending(),
    )
}

fn page_ordering_with_room_for(commands: usize) -> (LooperPage, CommandReceiver) {
    let (sending, orders) = command_channel(commands);

    (
        LooperPage::new(
            position_meter().1,
            waveform_meter().1,
            sample_clock(SECOND).1,
            sending,
        ),
        orders,
    )
}

fn page_ordering() -> (LooperPage, CommandReceiver) {
    page_ordering_with_room_for(ROOM)
}

fn page_showing(position: LoopPosition) -> LooperPage {
    let (mut writer, reader) = position_meter();
    writer.publish(position);

    LooperPage::new(
        reader,
        waveform_meter().1,
        sample_clock(SECOND).1,
        sending(),
    )
}

fn page_stacked(depth: usize) -> LooperPage {
    page_showing(LoopPosition::new(SECOND, 8 * SECOND, depth))
}

fn page_on_a_clock_at(sample_rate: u32) -> (LooperPage, SampleClockWriter) {
    let (frames, elapsed) = sample_clock(sample_rate);

    (
        LooperPage::new(position_meter().1, waveform_meter().1, elapsed, sending()),
        frames,
    )
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

/// The glyphs the loop's shape is drawn from, densest last.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn recorded(samples: &[f32]) -> LoopWaveform {
    let mut waveform = LoopWaveform::EMPTY;
    waveform.take(0, samples.iter().copied());

    waveform
}

/// A loop at full scale from end to end, so every column of it is drawn as
/// tall as the page allows.
fn recorded_at_full_scale() -> LoopWaveform {
    recorded(&[1.0, -1.0].repeat(LoopWaveform::BUCKETS))
}

fn page_drawing(waveform: &LoopWaveform) -> LooperPage {
    let (mut writer, reader) = waveform_meter();
    writer.publish(waveform);

    LooperPage::new(
        position_meter().1,
        reader,
        sample_clock(SECOND).1,
        sending(),
    )
}

/// Which rows of the drawn page carry any of the loop's shape.
fn shape_rows(page: &mut LooperPage) -> Vec<usize> {
    drawn(page)
        .iter()
        .enumerate()
        .filter(|(_row, drawn)| drawn.chars().any(|glyph| BLOCKS.contains(&glyph)))
        .map(|(row, _drawn)| row)
        .collect()
}

fn row_starting_with(page: &mut LooperPage, opening: &str) -> usize {
    drawn(page)
        .iter()
        .position(|drawn| drawn.starts_with(opening))
        .unwrap_or_else(|| panic!("the page draws a row opening {opening:?}"))
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
fn a_press_reaches_the_engine_as_the_transport_it_asked_for() {
    let (mut page, mut orders) = page_ordering();

    page.control(pressed(Button::Record));

    assert_eq!(
        ordered(&mut orders),
        [Command::SetTransport(Transport::Recording)]
    );
}

#[test]
fn every_press_that_moves_the_transport_is_ordered() {
    let (mut page, mut orders) = page_ordering();

    page.control(pressed(Button::Record));
    page.control(pressed(Button::Play));
    page.control(pressed(Button::Stop));

    assert_eq!(
        ordered(&mut orders),
        [
            Command::SetTransport(Transport::Recording),
            Command::SetTransport(Transport::Playing),
            Command::SetTransport(Transport::Stopped),
        ]
    );
}

#[test]
fn a_page_nobody_has_pressed_orders_nothing() {
    let (mut page, mut orders) = page_ordering();

    drawn(&mut page);

    assert_eq!(ordered(&mut orders), []);
}

#[test]
fn a_transport_the_engine_already_has_is_not_ordered_again() {
    let (mut page, mut orders) = page_ordering();
    page.control(pressed(Button::Record));
    ordered(&mut orders);

    drawn(&mut page);
    drawn(&mut page);

    assert_eq!(ordered(&mut orders), []);
}

#[test]
fn a_press_the_queue_had_no_room_for_is_ordered_on_the_next_frame() {
    let (mut page, mut orders) = page_ordering_with_room_for(1);

    page.control(pressed(Button::Record));
    page.control(pressed(Button::Play));

    assert_eq!(
        ordered(&mut orders),
        [Command::SetTransport(Transport::Recording)]
    );
    drawn(&mut page);
    assert_eq!(
        ordered(&mut orders),
        [Command::SetTransport(Transport::Playing)]
    );
}

#[test]
fn a_button_the_transport_ignores_orders_nothing() {
    let (mut page, mut orders) = page_ordering();

    page.control(pressed(Button::Up));
    page.control(pressed(Button::FirstScene));

    assert_eq!(ordered(&mut orders), []);
}

#[test]
fn a_tap_orders_nothing() {
    let (mut page, mut orders) = page_ordering();

    page.control(shifted(Button::Play));

    assert_eq!(ordered(&mut orders), []);
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
    let mut page = page_showing(LoopPosition::new(3 * SECOND / 2, 8 * SECOND, A_TAKE));

    assert!(screen_of(&mut page).contains("0:01.5 / 0:08.0"));
}

#[test]
fn a_loop_past_a_minute_reads_in_minutes() {
    let mut page = page_showing(LoopPosition::new(75 * SECOND, 90 * SECOND, A_TAKE));

    assert!(screen_of(&mut page).contains("1:15.0 / 1:30.0"));
}

#[test]
fn an_empty_loop_reads_as_nothing_recorded() {
    assert!(screen_of(&mut page()).contains("0:00.0 / 0:00.0"));
}

#[test]
fn the_bar_fills_with_the_playhead() {
    let quarter = bar_of(&mut page_showing(LoopPosition::new(
        2 * SECOND,
        8 * SECOND,
        A_TAKE,
    )));

    assert_eq!(quarter.filled * 4, quarter.width);
}

#[test]
fn a_finished_loop_fills_the_bar() {
    let bar = bar_of(&mut page_showing(LoopPosition::new(
        8 * SECOND,
        8 * SECOND,
        A_TAKE,
    )));

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
fn a_loop_with_nothing_recorded_shows_an_empty_stack() {
    let expected = format!("LAYERS 0/{}", LoopBuffer::LAYERS);

    assert!(screen_of(&mut page()).contains(&expected));
}

#[test]
fn the_stack_shows_how_many_layers_are_recorded() {
    let recorded = 3;
    let expected = format!("LAYERS {recorded}/{}", LoopBuffer::LAYERS);

    assert!(screen_of(&mut page_stacked(recorded)).contains(&expected));
}

#[test]
fn a_stack_with_no_room_left_shows_every_layer_taken() {
    let expected = format!("LAYERS {0}/{0}", LoopBuffer::LAYERS);

    assert!(screen_of(&mut page_stacked(LoopBuffer::LAYERS)).contains(&expected));
}

#[test]
fn the_stack_is_drawn_beside_the_gain_rather_than_over_it() {
    let mut page = page_stacked(3);
    let screen = screen_of(&mut page);

    assert!(screen.contains("IN"), "the stack landed on the gain row");
    assert!(screen.contains("LAYERS"));
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
fn turning_the_encoder_orders_the_gain_it_reached() {
    let (mut page, mut orders) = page_ordering();

    page.control(turned(Turn::Clockwise));

    assert_eq!(ordered(&mut orders), [Command::SetGain(page.gain())]);
}

#[test]
fn every_turn_that_moves_the_gain_is_ordered() {
    let (mut page, mut orders) = page_ordering();

    page.control(turned(Turn::Clockwise));
    page.control(turned(Turn::Clockwise));
    page.control(turned(Turn::Anticlockwise));

    assert_eq!(ordered(&mut orders).len(), 3);
}

#[test]
fn a_gain_the_engine_already_has_is_not_ordered_again() {
    let (mut page, mut orders) = page_ordering();
    page.control(turned(Turn::Clockwise));
    ordered(&mut orders);

    drawn(&mut page);
    drawn(&mut page);

    assert_eq!(ordered(&mut orders), []);
}

#[test]
fn a_turn_against_the_end_of_the_range_orders_nothing() {
    let (mut page, mut orders) = page_ordering_with_room_for(ROOM_FOR_THE_WHOLE_RANGE);
    for _ in 0..TURNS_PAST_THE_CEILING {
        page.control(turned(Turn::Clockwise));
    }
    ordered(&mut orders);

    page.control(turned(Turn::Clockwise));

    assert_eq!(ordered(&mut orders), []);
}

#[test]
fn a_turn_the_queue_had_no_room_for_is_ordered_on_the_next_frame() {
    let (mut page, mut orders) = page_ordering_with_room_for(1);

    page.control(pressed(Button::Record));
    page.control(turned(Turn::Clockwise));

    assert_eq!(
        ordered(&mut orders),
        [Command::SetTransport(Transport::Recording)]
    );
    drawn(&mut page);
    assert_eq!(ordered(&mut orders), [Command::SetGain(page.gain())]);
}

#[test]
fn shift_and_record_orders_the_mute() {
    let (mut page, mut orders) = page_ordering();

    page.control(shifted(Button::Record));

    assert_eq!(ordered(&mut orders), [Command::SetMuted(true)]);
}

#[test]
fn unmuting_is_ordered_too() {
    let (mut page, mut orders) = page_ordering();
    page.control(shifted(Button::Record));
    ordered(&mut orders);

    page.control(shifted(Button::Record));

    assert_eq!(ordered(&mut orders), [Command::SetMuted(false)]);
}

#[test]
fn a_mute_the_engine_already_has_is_not_ordered_again() {
    let (mut page, mut orders) = page_ordering();
    page.control(shifted(Button::Record));
    ordered(&mut orders);

    drawn(&mut page);
    drawn(&mut page);

    assert_eq!(ordered(&mut orders), []);
}

#[test]
fn a_mute_the_queue_had_no_room_for_is_ordered_on_the_next_frame() {
    let (mut page, mut orders) = page_ordering_with_room_for(1);

    page.control(pressed(Button::Record));
    page.control(shifted(Button::Record));

    assert_eq!(
        ordered(&mut orders),
        [Command::SetTransport(Transport::Recording)]
    );
    drawn(&mut page);
    assert_eq!(ordered(&mut orders), [Command::SetMuted(true)]);
}

#[test]
fn turning_the_encoder_leaves_the_transport_alone() {
    let mut page = driven_by([Button::Record]);

    page.control(turned(Turn::Clockwise));

    assert_eq!(page.transport(), Transport::Recording);
}

#[test]
fn an_empty_loop_draws_no_shape() {
    assert!(shape_rows(&mut page()).is_empty());
}

#[test]
fn a_recorded_loop_is_drawn_under_the_playhead() {
    let mut page = page_drawing(&recorded_at_full_scale());
    let bar = row_starting_with(&mut page, "[");

    assert!(
        shape_rows(&mut page).iter().all(|row| *row > bar),
        "the loop is drawn over the bar rather than under it"
    );
}

#[test]
fn a_full_scale_loop_is_drawn_the_whole_height_of_its_rows() {
    let mut page = page_drawing(&recorded_at_full_scale());

    assert_eq!(shape_rows(&mut page).len(), 4);
}

#[test]
fn a_quiet_loop_is_drawn_shorter_than_a_loud_one() {
    let quiet = recorded(&[0.125, -0.125].repeat(LoopWaveform::BUCKETS));
    let loud = shape_rows(&mut page_drawing(&recorded_at_full_scale())).len();

    assert!(shape_rows(&mut page_drawing(&quiet)).len() < loud);
}

#[test]
fn the_gain_readout_sits_directly_under_the_loop() {
    let mut page = page_drawing(&recorded_at_full_scale());
    let gain = row_starting_with(&mut page, "IN");
    let lowest = *shape_rows(&mut page)
        .last()
        .expect("the page draws the loop");

    assert_eq!(gain, lowest + 1);
}
