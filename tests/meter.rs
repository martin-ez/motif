//! The input level meter: what its bar says about a block of audio, and how
//! long a peak stays up after the block that made it has gone.
//!
//! Read back as the string the meter draws rather than off a frame, because
//! where the bar lands is the page's business and not the widget's. What the
//! widget promises is the bar.

use motif::audio::Levels;
use motif::device::DeviceProfile;
use motif::ui::{LevelMeter, columns_of};

const COLUMNS: usize = 12;
const SCALE: usize = COLUMNS - 2;

/// The amplitude [`LevelMeter::FLOOR_DBFS`] names, as a sample would carry it.
const AT_THE_FLOOR: f32 = 0.001;

/// A second of frames, which is how long the meter holds a peak for.
///
/// Taken from the screen rather than from the meter, so that the frames a test
/// draws stay a number however the meter works out its own: a test looping
/// [`LevelMeter::PEAK_HOLD_FRAMES`] times would run until the heat death of the
/// universe the moment that const came out wrong, instead of failing.
const HOLD_FRAMES: usize = DeviceProfile::TARGET.screen.refresh_rate as usize;

fn levels(peak: f32, rms: f32) -> Levels {
    Levels { peak, rms }
}

fn loud() -> Levels {
    levels(1.0, 1.0)
}

fn bar(meter: &mut LevelMeter, levels: Levels) -> String {
    meter.bar(levels, COLUMNS)
}

fn after(meter: &mut LevelMeter, levels: Levels, frames: usize) -> String {
    let mut drawn = String::new();

    for _ in 0..frames {
        drawn = bar(meter, levels);
    }

    drawn
}

fn cells(bar: &str) -> &str {
    bar.trim_start_matches('[').trim_end_matches(']')
}

fn filled(bar: &str) -> usize {
    cells(bar).chars().filter(|glyph| *glyph == '#').count()
}

fn marker(bar: &str) -> Option<usize> {
    cells(bar).chars().position(|glyph| glyph == '|')
}

#[test]
fn a_meter_with_nothing_to_show_draws_an_empty_bar() {
    assert_eq!(bar(&mut LevelMeter::new(), Levels::SILENT), "[----------]");
}

#[test]
fn a_new_meter_shows_the_same_as_a_default_one() {
    assert_eq!(
        bar(&mut LevelMeter::new(), Levels::SILENT),
        bar(&mut LevelMeter::default(), Levels::SILENT)
    );
}

#[test]
fn a_full_scale_block_fills_the_bar() {
    assert_eq!(bar(&mut LevelMeter::new(), loud()), "[#########|]");
}

#[test]
fn a_block_past_full_scale_fills_the_bar_and_no_more() {
    assert_eq!(
        bar(&mut LevelMeter::new(), levels(4.0, 4.0)),
        "[#########|]"
    );
}

#[test]
fn the_bar_follows_the_rms_and_the_marker_the_peak() {
    let drawn = bar(&mut LevelMeter::new(), levels(1.0, AT_THE_FLOOR));

    assert_eq!(filled(&drawn), 0);
    assert_eq!(marker(&drawn), Some(SCALE - 1));
}

#[test]
fn the_scale_is_in_decibels_and_not_in_amplitude() {
    let drawn = bar(&mut LevelMeter::new(), levels(0.5, 0.5));

    assert_eq!(drawn, "[########|-]", "half amplitude is 6 dB down");
}

#[test]
fn the_bar_falls_by_the_same_cells_for_the_same_decibels() {
    let drawn = bar(&mut LevelMeter::new(), levels(0.1, 0.1));

    assert_eq!(drawn, "[######|---]", "a tenth of full scale is 20 dB down");
}

#[test]
fn a_block_at_the_floor_draws_nothing() {
    let drawn = bar(&mut LevelMeter::new(), levels(AT_THE_FLOOR, AT_THE_FLOOR));

    assert_eq!(drawn, "[----------]");
}

#[test]
fn a_block_under_the_floor_draws_nothing() {
    let drawn = bar(&mut LevelMeter::new(), levels(0.000_01, 0.000_01));

    assert_eq!(drawn, "[----------]");
}

#[test]
fn a_peak_outstays_the_block_it_came_from() {
    let mut meter = LevelMeter::new();
    bar(&mut meter, loud());

    let drawn = bar(&mut meter, Levels::SILENT);

    assert_eq!(filled(&drawn), 0);
    assert_eq!(marker(&drawn), Some(SCALE - 1));
}

#[test]
fn a_peak_is_held_for_at_least_the_hold() {
    let mut meter = LevelMeter::new();
    bar(&mut meter, loud());

    let drawn = after(&mut meter, Levels::SILENT, HOLD_FRAMES);

    assert_eq!(marker(&drawn), Some(SCALE - 1));
}

#[test]
fn a_held_peak_is_gone_within_two_holds() {
    let mut meter = LevelMeter::new();
    bar(&mut meter, loud());

    let drawn = after(&mut meter, Levels::SILENT, 2 * HOLD_FRAMES);

    assert_eq!(marker(&drawn), None);
}

#[test]
fn a_louder_block_takes_the_marker_from_the_one_held() {
    let mut meter = LevelMeter::new();
    bar(&mut meter, levels(AT_THE_FLOOR, AT_THE_FLOOR));

    let drawn = bar(&mut meter, loud());

    assert_eq!(marker(&drawn), Some(SCALE - 1));
}

#[test]
fn a_quieter_block_leaves_the_marker_where_it_was() {
    let mut meter = LevelMeter::new();
    bar(&mut meter, loud());

    let drawn = bar(&mut meter, levels(0.01, 0.01));

    assert_eq!(marker(&drawn), Some(SCALE - 1));
}

#[test]
fn a_meter_fills_the_columns_it_was_given() {
    let drawn = bar(&mut LevelMeter::new(), loud());

    assert_eq!(columns_of(&drawn), COLUMNS);
}

#[test]
fn a_meter_with_no_room_for_its_brackets_draws_nothing() {
    let mut meter = LevelMeter::new();

    assert_eq!(meter.bar(loud(), 1), "");
    assert_eq!(meter.bar(loud(), 0), "");
}

#[test]
fn a_meter_with_room_for_only_its_brackets_draws_them() {
    assert_eq!(LevelMeter::new().bar(loud(), 2), "[]");
}

#[test]
fn the_hold_spans_a_second_of_frames() {
    assert_eq!(LevelMeter::PEAK_HOLD_FRAMES, HOLD_FRAMES);
}
