//! The bordered viewport: where the panel's edges land in a larger terminal.
//!
//! Like `terminal.rs`, this file knows what an escape sequence is, because the
//! backend is the one place allowed to know. What it is really asserting is
//! that the box is the profile's size and sits clear of the frame — the border
//! belongs to the terminal, and the panel it stands for has none.

use std::io::{self, Write};

use motif::device::DeviceProfile;
use motif::ui::{Cell, Frame, RenderError, Renderer, Viewport};

const SCREEN: motif::device::ScreenProfile = DeviceProfile::TARGET.screen;

/// A sink that refuses its first write and accepts everything after it,
/// standing in for a screen that came back.
struct FailsOnce {
    writes: usize,
    written: Vec<u8>,
}

impl FailsOnce {
    fn new() -> Self {
        Self {
            writes: 0,
            written: Vec::new(),
        }
    }
}

impl Write for FailsOnce {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if self.writes == 1 {
            return Err(io::Error::other("the screen is gone"));
        }
        self.written.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

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
    let mut viewport = Viewport::new(Vec::new());
    viewport.render(frame).expect("a vec accepts every write");

    String::from_utf8(viewport.sink().clone()).expect("the output is utf-8")
}

fn render_after(first: &Frame, second: &Frame) -> String {
    let mut viewport = Viewport::new(Vec::new());
    viewport.render(first).expect("a vec accepts every write");
    let already_written = viewport.sink().len();
    viewport.render(second).expect("a vec accepts every write");

    String::from_utf8(viewport.sink()[already_written..].to_vec()).expect("the output is utf-8")
}

fn drawn(cells: &[(usize, usize, char)]) -> Frame {
    let mut frame = Frame::blank();
    for (column, row, glyph) in cells {
        frame.set(*column, *row, Cell::new(*glyph));
    }
    frame
}

fn top_border() -> String {
    format!("\u{1b}[1;1H┌{}┐", "─".repeat(SCREEN.columns))
}

fn bottom_border() -> String {
    format!(
        "\u{1b}[{};1H└{}┘",
        SCREEN.rows + 2,
        "─".repeat(SCREEN.columns)
    )
}

#[test]
fn a_viewport_draws_a_border_before_its_first_frame() {
    let output = first_render(&Frame::blank());

    assert!(
        output.starts_with(&top_border()),
        "the border was not the first thing drawn: {output:?}"
    );
}

#[test]
fn the_border_is_one_cell_clear_of_the_screen_on_every_side() {
    let output = first_render(&Frame::blank());

    assert!(output.contains(&top_border()), "no top border");
    assert!(output.contains(&bottom_border()), "no bottom border");

    for row in 2..=SCREEN.rows + 1 {
        assert!(
            output.contains(&format!("\u{1b}[{row};1H│")),
            "row {row} has no left edge"
        );
        assert!(
            output.contains(&format!("\u{1b}[{};{}H│", row, SCREEN.columns + 2)),
            "row {row} has no right edge"
        );
    }
}

#[test]
fn the_top_left_cell_is_drawn_inside_the_border() {
    let output = render_after(&Frame::blank(), &drawn(&[(0, 0, 'x')]));

    assert_eq!(output, "\u{1b}[2;2Hx");
}

#[test]
fn a_frame_that_did_not_change_writes_nothing() {
    let output = render_after(&Frame::blank(), &Frame::blank());

    assert_eq!(output, "");
}

#[test]
fn the_border_is_not_drawn_again_on_a_later_frame() {
    let output = render_after(&Frame::blank(), &drawn(&[(3, 2, 'x')]));

    assert_eq!(output, "\u{1b}[4;5Hx");
}

#[test]
fn a_screen_that_refuses_a_write_is_an_error() {
    let mut viewport = Viewport::new(BrokenSink);

    let outcome = viewport.render(&Frame::blank());

    assert_eq!(outcome, Err(RenderError::WriteFailed));
}

#[test]
fn the_border_is_drawn_again_after_a_failed_write() {
    let mut viewport = Viewport::new(FailsOnce::new());
    let failed = viewport.render(&Frame::blank());

    let recovered = viewport.render(&Frame::blank());

    assert_eq!(failed, Err(RenderError::WriteFailed));
    assert_eq!(recovered, Ok(()));

    let output = String::from_utf8(viewport.sink().written.clone()).expect("the output is utf-8");
    assert!(
        output.contains(&top_border()),
        "the border was not redrawn: {output:?}"
    );
}
