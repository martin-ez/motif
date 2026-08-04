//! Where the loop has got to, published by the callback and read by the screen.
//!
//! The pair is the point: a playhead means nothing without the length it is a
//! position within, so every test here reads both and the two are never checked
//! apart.

use motif::looper::{LoopPosition, position_meter};

const SECOND: u32 = 48_000;

#[test]
fn an_empty_loop_has_no_length() {
    assert_eq!(LoopPosition::EMPTY.recorded(), 0);
}

#[test]
fn an_empty_loop_has_no_playhead() {
    assert_eq!(LoopPosition::EMPTY.playhead(), 0);
}

#[test]
fn a_new_meter_reads_an_empty_loop() {
    let (_writer, reader) = position_meter();

    assert_eq!(reader.read(), LoopPosition::EMPTY);
}

#[test]
fn a_published_position_reads_back() {
    let (mut writer, reader) = position_meter();

    writer.publish(LoopPosition::new(SECOND, 4 * SECOND));

    assert_eq!(reader.read(), LoopPosition::new(SECOND, 4 * SECOND));
}

#[test]
fn reading_leaves_the_position_where_it_is() {
    let (mut writer, reader) = position_meter();
    writer.publish(LoopPosition::new(SECOND, 4 * SECOND));

    let read = reader.read();

    assert_eq!(reader.read(), read);
}

#[test]
fn the_newest_publication_replaces_the_one_nobody_read() {
    let (mut writer, reader) = position_meter();

    writer.publish(LoopPosition::new(SECOND, 4 * SECOND));
    writer.publish(LoopPosition::new(2 * SECOND, 4 * SECOND));

    assert_eq!(reader.read(), LoopPosition::new(2 * SECOND, 4 * SECOND));
}

#[test]
fn a_playhead_past_the_loop_is_held_at_its_end() {
    let position = LoopPosition::new(9 * SECOND, 4 * SECOND);

    assert_eq!(position.playhead(), 4 * SECOND);
}

#[test]
fn a_playhead_in_a_loop_of_nothing_is_nothing() {
    let position = LoopPosition::new(SECOND, 0);

    assert_eq!(position.playhead(), 0);
}

#[test]
fn the_longest_representable_loop_reads_back() {
    let (mut writer, reader) = position_meter();
    let full = LoopPosition::new(u32::MAX, u32::MAX);

    writer.publish(full);

    assert_eq!(reader.read(), full);
}
