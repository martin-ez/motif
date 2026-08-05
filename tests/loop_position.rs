//! Where the loop has got to, published by the callback and read by the screen.
//!
//! The three are the point: a playhead means nothing without the length it is a
//! position within, and a depth read a block apart from either would say the
//! loop has layers it has no samples for. Every test here reads what it
//! published whole, and the three are never checked apart.

use motif::looper::{LoopBuffer, LoopPosition, position_meter};

const SECOND: u32 = 48_000;
const NOTHING_RECORDED: usize = 0;
const A_TAKE: usize = 1;
const A_TAKE_AND_AN_OVERDUB: usize = 2;

#[test]
fn an_empty_loop_has_no_length() {
    assert_eq!(LoopPosition::EMPTY.recorded(), 0);
}

#[test]
fn an_empty_loop_has_no_playhead() {
    assert_eq!(LoopPosition::EMPTY.playhead(), 0);
}

#[test]
fn an_empty_loop_has_no_layers() {
    assert_eq!(LoopPosition::EMPTY.depth(), 0);
}

#[test]
fn a_new_meter_reads_an_empty_loop() {
    let (_writer, reader) = position_meter();

    assert_eq!(reader.read(), LoopPosition::EMPTY);
}

#[test]
fn a_published_position_reads_back() {
    let (mut writer, reader) = position_meter();
    let published = LoopPosition::new(SECOND, 4 * SECOND, A_TAKE);

    writer.publish(published);

    assert_eq!(reader.read(), published);
}

#[test]
fn reading_leaves_the_position_where_it_is() {
    let (mut writer, reader) = position_meter();
    writer.publish(LoopPosition::new(SECOND, 4 * SECOND, A_TAKE));

    let read = reader.read();

    assert_eq!(reader.read(), read);
}

#[test]
fn the_newest_publication_replaces_the_one_nobody_read() {
    let (mut writer, reader) = position_meter();

    writer.publish(LoopPosition::new(SECOND, 4 * SECOND, A_TAKE));
    writer.publish(LoopPosition::new(
        2 * SECOND,
        4 * SECOND,
        A_TAKE_AND_AN_OVERDUB,
    ));

    assert_eq!(
        reader.read(),
        LoopPosition::new(2 * SECOND, 4 * SECOND, A_TAKE_AND_AN_OVERDUB)
    );
}

#[test]
fn a_playhead_past_the_loop_is_held_at_its_end() {
    let position = LoopPosition::new(9 * SECOND, 4 * SECOND, A_TAKE);

    assert_eq!(position.playhead(), 4 * SECOND);
}

#[test]
fn a_playhead_in_a_loop_of_nothing_is_nothing() {
    let position = LoopPosition::new(SECOND, 0, NOTHING_RECORDED);

    assert_eq!(position.playhead(), 0);
}

#[test]
fn a_depth_past_the_stack_is_held_at_its_top() {
    let position = LoopPosition::new(SECOND, 4 * SECOND, LoopBuffer::LAYERS + 1);

    assert_eq!(position.depth(), LoopBuffer::LAYERS);
}

#[test]
fn a_frame_count_past_the_ceiling_is_held_at_it() {
    let position = LoopPosition::new(u32::MAX, u32::MAX, A_TAKE);

    assert_eq!(position.recorded(), LoopPosition::MAX_FRAMES);
}

#[test]
fn the_longest_representable_loop_reads_back() {
    let (mut writer, reader) = position_meter();
    let full = LoopPosition::new(
        LoopPosition::MAX_FRAMES,
        LoopPosition::MAX_FRAMES,
        LoopBuffer::LAYERS,
    );

    writer.publish(full);

    assert_eq!(reader.read(), full);
}

#[test]
fn the_deepest_stack_does_not_reach_into_the_loop_it_is_published_beside() {
    let deepest = LoopPosition::new(SECOND, 4 * SECOND, LoopBuffer::LAYERS);

    assert_eq!(deepest.playhead(), SECOND);
    assert_eq!(deepest.recorded(), 4 * SECOND);
}

#[test]
fn the_longest_loop_does_not_reach_into_the_stack_published_beside_it() {
    let longest = LoopPosition::new(
        LoopPosition::MAX_FRAMES,
        LoopPosition::MAX_FRAMES,
        A_TAKE_AND_AN_OVERDUB,
    );

    assert_eq!(longest.depth(), A_TAKE_AND_AN_OVERDUB);
}
