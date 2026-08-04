//! The terminal backend: what it writes, and how little of it.
//!
//! This is the one test file that knows what an escape sequence is, because it
//! tests the one file allowed to know.

use std::io::{self, Write};

use motif::device::DeviceProfile;
use motif::ui::{Cell, Frame, FrameWriter, RenderError, Renderer};

const SCREEN: motif::device::ScreenProfile = DeviceProfile::TARGET.screen;

/// A sink that refuses everything, standing in for a screen that has gone away.
struct BrokenSink;

impl Write for BrokenSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("the screen is gone"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn first_render(frame: &Frame) -> String {
    let mut writer = FrameWriter::new(Vec::new());
    writer.render(frame).expect("a vec accepts every write");

    String::from_utf8(writer.sink().clone()).expect("the output is utf-8")
}

fn render_after(first: &Frame, second: &Frame) -> String {
    let mut writer = FrameWriter::new(Vec::new());
    writer.render(first).expect("a vec accepts every write");
    let already_written = writer.sink().len();
    writer.render(second).expect("a vec accepts every write");

    String::from_utf8(writer.sink()[already_written..].to_vec()).expect("the output is utf-8")
}

fn drawn(cells: &[(usize, usize, char)]) -> Frame {
    let mut frame = Frame::blank();
    for (column, row, glyph) in cells {
        frame.set(*column, *row, Cell::new(*glyph));
    }
    frame
}

#[test]
fn the_first_frame_positions_every_row() {
    let output = first_render(&Frame::blank());

    for row in 1..=SCREEN.rows {
        assert!(
            output.contains(&format!("\u{1b}[{row};1H")),
            "row {row} was never positioned"
        );
    }
}

#[test]
fn the_first_frame_writes_a_cell_for_every_column() {
    let output = first_render(&Frame::blank());

    let glyphs = output.chars().filter(|glyph| *glyph == ' ').count();

    assert_eq!(glyphs, SCREEN.cells());
}

#[test]
fn a_frame_that_did_not_change_writes_nothing() {
    let output = render_after(&Frame::blank(), &Frame::blank());

    assert_eq!(output, "");
}

#[test]
fn only_the_cell_that_changed_is_written() {
    let output = render_after(&Frame::blank(), &drawn(&[(3, 2, 'x')]));

    assert_eq!(output, "\u{1b}[3;4Hx");
}

#[test]
fn a_position_counts_from_one_not_zero() {
    let output = render_after(&Frame::blank(), &drawn(&[(0, 0, 'x')]));

    assert_eq!(output, "\u{1b}[1;1Hx");
}

#[test]
fn changed_cells_side_by_side_are_one_positioned_run() {
    let output = render_after(&Frame::blank(), &drawn(&[(3, 2, 'x'), (4, 2, 'y')]));

    assert_eq!(output, "\u{1b}[3;4Hxy");
}

#[test]
fn a_run_of_changes_keeps_growing_past_the_second_cell() {
    let output = render_after(
        &Frame::blank(),
        &drawn(&[(3, 2, 'x'), (4, 2, 'y'), (5, 2, 'z')]),
    );

    assert_eq!(output, "\u{1b}[3;4Hxyz");
}

#[test]
fn a_gap_between_changes_starts_a_new_run() {
    let output = render_after(&Frame::blank(), &drawn(&[(3, 2, 'x'), (6, 2, 'y')]));

    assert_eq!(output, "\u{1b}[3;4Hx\u{1b}[3;7Hy");
}

#[test]
fn a_change_on_another_row_is_positioned_again() {
    let output = render_after(&Frame::blank(), &drawn(&[(3, 2, 'x'), (3, 4, 'y')]));

    assert_eq!(output, "\u{1b}[3;4Hx\u{1b}[5;4Hy");
}

#[test]
fn a_cell_that_reverts_is_written_back() {
    let mut writer = FrameWriter::new(Vec::new());
    writer
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    writer
        .render(&drawn(&[(3, 2, 'x')]))
        .expect("a vec accepts every write");
    let already_written = writer.sink().len();
    writer
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    let output =
        String::from_utf8(writer.sink()[already_written..].to_vec()).expect("the output is utf-8");

    assert_eq!(output, "\u{1b}[3;4H ");
}

#[test]
fn a_writer_given_an_origin_offsets_every_position() {
    let mut writer = FrameWriter::at(Vec::new(), 1, 1);
    writer
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = writer.sink().len();
    writer
        .render(&drawn(&[(0, 0, 'x')]))
        .expect("a vec accepts every write");

    let output =
        String::from_utf8(writer.sink()[already_written..].to_vec()).expect("the output is utf-8");

    assert_eq!(output, "\u{1b}[2;2Hx");
}

#[test]
fn a_screen_that_refuses_a_write_is_an_error() {
    let mut writer = FrameWriter::new(BrokenSink);

    let outcome = writer.render(&Frame::blank());

    assert_eq!(outcome, Err(RenderError::WriteFailed));
}

#[test]
fn an_unavailable_screen_describes_itself() {
    assert_eq!(
        RenderError::Unavailable.to_string(),
        "the screen is not available"
    );
}
