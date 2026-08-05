//! A band of a frame, and what splitting one gives.
//!
//! A region is what a page draws into, so the properties that matter are the
//! ones a page can rely on: it is addressed from its own top left, it clips at
//! its own edges, and the two halves of a split cannot reach each other's cells.
//!
//! Nothing here names a terminal.

use motif::device::{DeviceProfile, ScreenProfile};
use motif::ui::{Cell, Frame, Region};

const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const WIDE: char = 'オ';
const MARK: char = '#';

fn filled(region: &mut Region<'_>, glyph: char) {
    for row in 0..region.rows() {
        for column in 0..region.columns() {
            region.set(column, row, Cell::new(glyph));
        }
    }
}

fn rows_of(frame: &Frame, glyph: char) -> Vec<usize> {
    (0..SCREEN.rows)
        .filter(|row| {
            (0..SCREEN.columns).any(|column| frame.get(column, *row) == Some(Cell::new(glyph)))
        })
        .collect()
}

#[test]
fn a_whole_frame_is_a_region_of_the_screens_size() {
    let mut frame = Frame::blank();
    let region = frame.region();

    assert_eq!(region.columns(), SCREEN.columns);
    assert_eq!(region.rows(), SCREEN.rows);
}

#[test]
fn a_cell_written_to_a_region_is_read_back_from_it() {
    let mut frame = Frame::blank();
    let mut region = frame.region();

    region.set(3, 2, Cell::new('x'));

    assert_eq!(region.get(3, 2), Some(Cell::new('x')));
}

#[test]
fn splitting_the_top_gives_the_rows_asked_for() {
    let mut frame = Frame::blank();
    let (top, rest) = frame.region().split_top(3);

    assert_eq!(top.rows(), 3);
    assert_eq!(rest.rows(), SCREEN.rows - 3);
}

#[test]
fn splitting_the_bottom_gives_the_rows_asked_for() {
    let mut frame = Frame::blank();
    let (rest, bottom) = frame.region().split_bottom(2);

    assert_eq!(bottom.rows(), 2);
    assert_eq!(rest.rows(), SCREEN.rows - 2);
}

#[test]
fn a_split_keeps_the_regions_width() {
    let mut frame = Frame::blank();
    let (top, rest) = frame.region().split_top(1);

    assert_eq!(top.columns(), SCREEN.columns);
    assert_eq!(rest.columns(), SCREEN.columns);
}

#[test]
fn the_remainder_of_a_top_split_starts_below_the_strip() {
    let mut frame = Frame::blank();
    let (_strip, mut rest) = frame.region().split_top(2);

    rest.set(0, 0, Cell::new(MARK));

    assert_eq!(frame.get(0, 2), Some(Cell::new(MARK)));
    assert_eq!(frame.get(0, 0), Some(Cell::BLANK));
}

#[test]
fn the_strip_of_a_bottom_split_starts_at_the_rows_it_took() {
    let mut frame = Frame::blank();
    let (_rest, mut strip) = frame.region().split_bottom(1);

    strip.set(0, 0, Cell::new(MARK));

    assert_eq!(frame.get(0, SCREEN.rows - 1), Some(Cell::new(MARK)));
}

#[test]
fn a_region_that_fills_itself_reaches_no_row_of_the_strip() {
    let mut frame = Frame::blank();
    let (mut rest, _strip) = frame.region().split_bottom(1);

    filled(&mut rest, MARK);

    assert_eq!(
        rows_of(&frame, MARK),
        (0..SCREEN.rows - 1).collect::<Vec<_>>()
    );
}

#[test]
fn a_split_covers_every_cell_of_the_frame_and_no_cell_twice() {
    let mut frame = Frame::blank();
    let (mut top, mut rest) = frame.region().split_top(4);
    filled(&mut top, 'a');
    filled(&mut rest, 'b');

    assert_eq!(rows_of(&frame, 'a'), (0..4).collect::<Vec<_>>());
    assert_eq!(rows_of(&frame, 'b'), (4..SCREEN.rows).collect::<Vec<_>>());
}

#[test]
fn a_write_past_a_regions_last_row_is_dropped() {
    let mut frame = Frame::blank();
    let (mut top, _rest) = frame.region().split_top(2);

    top.set(0, 2, Cell::new(MARK));

    assert!(rows_of(&frame, MARK).is_empty());
}

#[test]
fn a_write_past_a_regions_last_column_is_dropped() {
    let mut frame = Frame::blank();
    let mut region = frame.region();

    region.set(SCREEN.columns, 0, Cell::new(MARK));

    assert!(rows_of(&frame, MARK).is_empty());
}

#[test]
fn a_read_outside_a_region_is_nothing() {
    let mut frame = Frame::blank();
    let (top, _rest) = frame.region().split_top(2);

    assert_eq!(top.get(0, 2), None);
    assert_eq!(top.get(SCREEN.columns, 0), None);
}

#[test]
fn text_is_written_from_the_regions_own_top_left() {
    let mut frame = Frame::blank();
    let (_strip, mut rest) = frame.region().split_top(1);

    rest.write(2, 0, "ok");

    assert_eq!(frame.get(2, 1), Some(Cell::new('o')));
    assert_eq!(frame.get(3, 1), Some(Cell::new('k')));
}

#[test]
fn a_wide_glyph_claims_the_column_beside_it_in_a_region() {
    let mut frame = Frame::blank();
    let (_strip, mut rest) = frame.region().split_top(1);

    rest.set(3, 0, Cell::new(WIDE));

    let beside = rest.get(4, 0).expect("the column beside is in the region");
    assert_eq!(beside.columns(), 0);
}

#[test]
fn a_wide_glyph_with_no_column_beside_it_is_dropped_from_a_region() {
    let mut frame = Frame::blank();
    let mut region = frame.region();

    region.set(SCREEN.columns - 1, 0, Cell::new(WIDE));

    assert!(frame.cells().iter().all(|cell| *cell == Cell::BLANK));
}

#[test]
fn overwriting_the_column_beside_a_wide_glyph_clears_it_in_a_region() {
    let mut frame = Frame::blank();
    let (_strip, mut rest) = frame.region().split_top(1);
    rest.set(3, 0, Cell::new(WIDE));

    rest.set(4, 0, Cell::new('x'));

    assert_eq!(rest.get(3, 0), Some(Cell::BLANK));
}

#[test]
fn a_split_longer_than_the_region_leaves_no_remainder() {
    let mut frame = Frame::blank();
    let (top, rest) = frame.region().split_top(SCREEN.rows + 4);

    assert_eq!(top.rows(), SCREEN.rows);
    assert_eq!(rest.rows(), 0);
}

#[test]
fn a_bottom_split_longer_than_the_region_leaves_no_remainder() {
    let mut frame = Frame::blank();
    let (rest, bottom) = frame.region().split_bottom(SCREEN.rows + 4);

    assert_eq!(bottom.rows(), SCREEN.rows);
    assert_eq!(rest.rows(), 0);
}

#[test]
fn nothing_can_be_written_to_a_region_of_no_rows() {
    let mut frame = Frame::blank();
    let (_top, mut rest) = frame.region().split_top(SCREEN.rows);

    rest.set(0, 0, Cell::new(MARK));

    assert!(rows_of(&frame, MARK).is_empty());
}
