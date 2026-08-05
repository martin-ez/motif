//! The frame the UI draws into, and the backend it is handed to.
//!
//! Nothing here names a terminal, which is the property the abstraction exists
//! to have.

use motif::device::DeviceProfile;
use motif::ui::{Cell, Frame, NullRenderer, RenderError, Renderer, columns_of};

const WIDE: char = 'オ';
const LABEL: &str = "オーディオ";

const SCREEN: motif::device::ScreenProfile = DeviceProfile::TARGET.screen;

fn last_column() -> usize {
    SCREEN.columns - 1
}

fn last_row() -> usize {
    SCREEN.rows - 1
}

fn index_of(frame: &Frame, cell: Cell) -> isize {
    frame
        .cells()
        .iter()
        .position(|candidate| *candidate == cell)
        .expect("the cell was drawn somewhere") as isize
}

fn step_between(from: (usize, usize), to: (usize, usize)) -> isize {
    let mut frame = Frame::blank();
    frame.set(from.0, from.1, Cell::new('a'));
    frame.set(to.0, to.1, Cell::new('b'));

    index_of(&frame, Cell::new('b')) - index_of(&frame, Cell::new('a'))
}

#[test]
fn a_cell_shows_the_glyph_it_was_made_with() {
    assert_eq!(Cell::new('x').glyph(), 'x');
}

#[test]
fn a_blank_cell_shows_a_space() {
    assert_eq!(Cell::BLANK.glyph(), ' ');
}

#[test]
fn an_escape_character_is_not_a_glyph() {
    assert_eq!(Cell::new('\u{1b}'), Cell::BLANK);
}

#[test]
fn a_line_break_is_not_a_glyph() {
    assert_eq!(Cell::new('\n'), Cell::BLANK);
    assert_eq!(Cell::new('\r'), Cell::BLANK);
}

#[test]
fn a_blank_frame_holds_only_blank_cells() {
    let frame = Frame::blank();

    assert!(frame.cells().iter().all(|cell| *cell == Cell::BLANK));
}

#[test]
fn a_frame_covers_the_whole_screen() {
    let frame = Frame::blank();

    assert_eq!(frame.cells().len(), SCREEN.cells());
}

#[test]
fn a_cell_that_was_written_is_read_back() {
    let mut frame = Frame::blank();

    frame.set(3, 2, Cell::new('x'));

    assert_eq!(frame.get(3, 2), Some(Cell::new('x')));
}

#[test]
fn a_cell_is_addressed_by_column_before_row() {
    let mut frame = Frame::blank();

    frame.set(3, 2, Cell::new('x'));

    assert_eq!(frame.get(2, 3), Some(Cell::BLANK));
}

#[test]
fn the_cell_one_column_right_is_the_very_next_cell() {
    assert_eq!(step_between((3, 2), (4, 2)), 1);
}

#[test]
fn the_cell_one_row_down_is_a_whole_screen_width_on() {
    assert_eq!(step_between((3, 2), (3, 3)), SCREEN.columns as isize);
}

#[test]
fn the_last_cell_on_the_screen_is_writable() {
    let mut frame = Frame::blank();

    frame.set(last_column(), last_row(), Cell::new('x'));

    assert_eq!(frame.get(last_column(), last_row()), Some(Cell::new('x')));
}

#[test]
fn a_write_beyond_the_last_column_is_dropped() {
    let mut frame = Frame::blank();

    frame.set(SCREEN.columns, 0, Cell::new('x'));

    assert!(frame.cells().iter().all(|cell| *cell == Cell::BLANK));
}

#[test]
fn a_write_beyond_the_last_row_is_dropped() {
    let mut frame = Frame::blank();

    frame.set(0, SCREEN.rows, Cell::new('x'));

    assert!(frame.cells().iter().all(|cell| *cell == Cell::BLANK));
}

#[test]
fn a_read_outside_the_screen_is_nothing() {
    let frame = Frame::blank();

    assert_eq!(frame.get(SCREEN.columns, 0), None);
    assert_eq!(frame.get(0, SCREEN.rows), None);
}

#[test]
fn a_renderer_is_handed_the_frame_that_was_drawn() {
    let mut renderer = NullRenderer::new();
    let mut frame = Frame::blank();
    frame.set(1, 1, Cell::new('m'));

    renderer.render(&frame).expect("the null renderer renders");

    let rendered = renderer.rendered().expect("a frame was rendered");
    assert_eq!(rendered.get(1, 1), Some(Cell::new('m')));
}

#[test]
fn a_renderer_holds_nothing_before_it_renders() {
    let renderer = NullRenderer::new();

    assert!(renderer.rendered().is_none());
}

#[test]
fn a_renderer_keeps_only_the_latest_frame() {
    let mut renderer = NullRenderer::new();
    let mut first = Frame::blank();
    first.set(0, 0, Cell::new('a'));
    let mut second = Frame::blank();
    second.set(0, 0, Cell::new('b'));

    renderer.render(&first).expect("the null renderer renders");
    renderer
        .render(&second)
        .expect("the null renderer renders again");

    let rendered = renderer.rendered().expect("a frame was rendered");
    assert_eq!(rendered.get(0, 0), Some(Cell::new('b')));
}

#[test]
fn an_ascii_glyph_costs_one_column() {
    assert_eq!(Cell::new('x').columns(), 1);
}

#[test]
fn a_wide_glyph_costs_two_columns() {
    assert_eq!(Cell::new(WIDE).columns(), 2);
}

#[test]
fn a_combining_mark_is_not_a_glyph() {
    assert_eq!(Cell::new('\u{0301}'), Cell::BLANK);
}

#[test]
fn the_column_beside_a_wide_glyph_belongs_to_it() {
    let mut frame = Frame::blank();

    frame.set(3, 2, Cell::new(WIDE));

    let beside = frame.get(4, 2).expect("the column beside is on the screen");
    assert_ne!(beside, Cell::BLANK);
    assert_eq!(beside.columns(), 0);
}

#[test]
fn a_wide_glyph_with_no_column_beside_it_is_dropped() {
    let mut frame = Frame::blank();

    frame.set(last_column(), 0, Cell::new(WIDE));

    assert!(frame.cells().iter().all(|cell| *cell == Cell::BLANK));
}

#[test]
fn overwriting_a_wide_glyph_clears_the_column_beside_it() {
    let mut frame = Frame::blank();
    frame.set(3, 2, Cell::new(WIDE));

    frame.set(3, 2, Cell::new('x'));

    assert_eq!(frame.get(4, 2), Some(Cell::BLANK));
}

#[test]
fn overwriting_the_column_beside_a_wide_glyph_clears_the_glyph() {
    let mut frame = Frame::blank();
    frame.set(3, 2, Cell::new(WIDE));

    frame.set(4, 2, Cell::new('x'));

    assert_eq!(frame.get(3, 2), Some(Cell::BLANK));
}

#[test]
fn a_wide_glyph_landing_on_another_leaves_no_half_behind() {
    let mut frame = Frame::blank();
    frame.set(4, 2, Cell::new(WIDE));

    frame.set(3, 2, Cell::new(WIDE));

    assert_eq!(frame.get(5, 2), Some(Cell::BLANK));
}

#[test]
fn text_is_written_from_the_column_it_was_given() {
    let mut frame = Frame::blank();

    frame.write(2, 1, "ok");

    assert_eq!(frame.get(2, 1), Some(Cell::new('o')));
    assert_eq!(frame.get(3, 1), Some(Cell::new('k')));
}

#[test]
fn a_wide_glyph_moves_the_text_after_it_two_columns_on() {
    let mut frame = Frame::blank();

    frame.write(0, 0, "オk");

    assert_eq!(frame.get(0, 0), Some(Cell::new(WIDE)));
    assert_eq!(frame.get(2, 0), Some(Cell::new('k')));
}

#[test]
fn a_wide_label_ends_where_its_columns_run_out() {
    let mut frame = Frame::blank();

    frame.write(0, 0, LABEL);

    assert_eq!(frame.get(10, 0), Some(Cell::BLANK));
}

#[test]
fn a_wide_label_stops_at_the_margin() {
    let mut frame = Frame::blank();

    frame.write(SCREEN.columns - 4, 0, LABEL);

    assert_eq!(frame.get(SCREEN.columns - 4, 0), Some(Cell::new(WIDE)));
    assert!((0..SCREEN.columns).all(|column| frame.get(column, 1) == Some(Cell::BLANK)));
}

#[test]
fn an_ascii_label_costs_one_column_a_glyph() {
    assert_eq!(columns_of("motif"), 5);
}

#[test]
fn a_wide_label_costs_two_columns_a_glyph() {
    assert_eq!(columns_of(LABEL), 10);
}

#[test]
fn a_render_error_describes_itself() {
    assert_eq!(
        RenderError::WriteFailed.to_string(),
        "the screen could not be written to"
    );
}
