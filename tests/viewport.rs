//! The bordered viewport: where the screen's edges and the keys under them land
//! in a larger terminal.
//!
//! Like `terminal.rs`, this file knows what an escape sequence is, because the
//! backend is the one place allowed to know. What it is really asserting is
//! that the box is the profile's size and sits clear of the frame — the border
//! belongs to the terminal, and the screen it stands for has none — and that
//! the keys the terminal draws for the panel it does not have stay outside it.

use std::cell::{Cell as MutableCell, RefCell};
use std::io::{self, Write};
use std::rc::Rc;

use motif::device::{Button, DeviceProfile};
use motif::ui::{Cell, Frame, Legend, Panel, RenderError, Renderer, ScriptedControls, Viewport};

const SCREEN: motif::device::ScreenProfile = DeviceProfile::TARGET.screen;

/// The first terminal line the panel is drawn on, counted from one, for a
/// viewport at the origin: the border's two rows and the screen's, and a row
/// left blank between the box and the keys.
const PANEL_LINE: usize = SCREEN.rows + 4;

/// The terminal column the panel starts in, counted from one, for a viewport at
/// the origin.
const PANEL_COLUMN: usize = (SCREEN.columns + 2 - Panel::COLUMNS) / 2 + 1;

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

/// A sink that can be made to refuse and then accept again from outside,
/// so that a write can be failed after the border has already gone out.
#[derive(Clone)]
struct Switchable {
    refusing: Rc<MutableCell<bool>>,
    written: Rc<RefCell<Vec<u8>>>,
}

impl Switchable {
    fn new() -> Self {
        Self {
            refusing: Rc::new(MutableCell::new(false)),
            written: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn refuse(&self, refusing: bool) {
        self.refusing.set(refusing);
    }

    fn since(&self, already_written: usize) -> String {
        String::from_utf8(self.written.borrow()[already_written..].to_vec())
            .expect("the output is utf-8")
    }

    fn len(&self) -> usize {
        self.written.borrow().len()
    }
}

impl Write for Switchable {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.refusing.get() {
            return Err(io::Error::other("the screen is gone"));
        }
        self.written.borrow_mut().extend_from_slice(bytes);
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

fn picture() -> Panel {
    Legend::blank()
        .answering(Button::Play)
        .picture(&ScriptedControls::new([]))
}

fn row_of(panel: &Panel, row: usize) -> String {
    (0..Panel::COLUMNS)
        .filter_map(|column| panel.get(column, row))
        .map(|cell| cell.glyph())
        .collect()
}

fn shown(panel: &Panel) -> String {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();
    viewport
        .show_panel(panel)
        .expect("a vec accepts every write");

    String::from_utf8(viewport.sink()[already_written..].to_vec()).expect("the output is utf-8")
}

fn top_border_at(column: usize, row: usize) -> String {
    format!(
        "\u{1b}[{};{}H┌{}┐",
        row + 1,
        column + 1,
        "─".repeat(SCREEN.columns)
    )
}

fn top_border() -> String {
    top_border_at(0, 0)
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

#[test]
fn the_border_is_drawn_again_after_a_frame_failed_under_it() {
    let screen = Switchable::new();
    let mut viewport = Viewport::new(screen.clone());
    viewport
        .render(&Frame::blank())
        .expect("the screen is accepting writes");

    screen.refuse(true);
    let failed = viewport.render(&drawn(&[(3, 2, 'x')]));

    screen.refuse(false);
    let already_written = screen.len();
    let recovered = viewport.render(&drawn(&[(3, 2, 'x')]));

    assert_eq!(failed, Err(RenderError::WriteFailed));
    assert_eq!(recovered, Ok(()));

    let output = screen.since(already_written);
    assert!(
        output.contains(&top_border()),
        "the border was not redrawn after the frame under it failed: {output:?}"
    );
}

#[test]
fn a_placed_viewport_draws_its_border_at_the_origin_it_was_given() {
    let mut viewport = Viewport::at(Vec::new(), 3, 2);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink().clone()).expect("the output is utf-8");

    assert!(
        output.contains(&top_border_at(3, 2)),
        "the border was not drawn where it was placed: {output:?}"
    );
}

#[test]
fn a_placed_viewport_draws_the_frame_inside_its_border() {
    let mut viewport = Viewport::at(Vec::new(), 3, 2);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();
    viewport
        .render(&drawn(&[(0, 0, 'x')]))
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");

    assert_eq!(output, "\u{1b}[4;5Hx");
}

#[test]
fn moving_a_viewport_draws_its_border_again_where_it_now_is() {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport.place(5, 1);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");

    assert!(
        output.contains(&top_border_at(5, 1)),
        "the border did not move: {output:?}"
    );
}

#[test]
fn moving_a_viewport_wipes_the_screen_it_left_behind() {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport.place(5, 1);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");

    assert!(
        output.contains("\u{1b}[2J"),
        "the old border was left on the screen: {output:?}"
    );
}

#[test]
fn a_viewport_moved_where_it_already_is_writes_nothing() {
    let mut viewport = Viewport::at(Vec::new(), 5, 1);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport.place(5, 1);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");

    assert_eq!(output, "");
}

#[test]
fn a_viewport_moved_along_one_axis_still_moves() {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport.place(5, 0);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");

    assert!(
        output.contains(&top_border_at(5, 0)),
        "a move that changed only the column was ignored: {output:?}"
    );
}

#[test]
fn a_viewport_draws_the_panel_under_its_border() {
    let panel = picture();

    let output = shown(&panel);

    assert!(
        output.contains(&format!(
            "\u{1b}[{PANEL_LINE};{PANEL_COLUMN}H{}",
            row_of(&panel, 0)
        )),
        "the panel is not under the border: {output:?}"
    );
}

#[test]
fn the_panel_is_drawn_clear_of_the_screen_and_its_border() {
    let output = shown(&picture());

    for line in 1..PANEL_LINE {
        assert!(
            !output.contains(&format!("\u{1b}[{line};")),
            "the panel was drawn on line {line}, which the box has: {output:?}"
        );
    }
}

#[test]
fn the_panel_ends_on_the_last_row_a_viewport_covers() {
    let panel = picture();

    let output = shown(&panel);

    assert!(
        output.contains(&format!(
            "\u{1b}[{};{PANEL_COLUMN}H{}",
            Viewport::<Vec<u8>>::ROWS,
            row_of(&panel, Panel::ROWS - 1)
        )),
        "the panel runs past what a viewport says it covers: {output:?}"
    );
}

#[test]
fn a_panel_that_did_not_change_is_not_drawn_again() {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    viewport
        .show_panel(&picture())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport
        .show_panel(&picture())
        .expect("a vec accepts every write");

    assert_eq!(viewport.sink().len(), already_written);
}

#[test]
fn a_panel_that_changed_is_drawn_again() {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    viewport
        .show_panel(&picture())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport
        .show_panel(&Legend::blank().picture(&ScriptedControls::new([])))
        .expect("a vec accepts every write");

    assert!(viewport.sink().len() > already_written);
}

#[test]
fn moving_a_viewport_draws_the_panel_again_where_it_now_is() {
    let panel = picture();
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    viewport
        .show_panel(&panel)
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();

    viewport.place(5, 1);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    viewport
        .show_panel(&panel)
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");
    assert!(
        output.contains(&format!(
            "\u{1b}[{};{}H{}",
            PANEL_LINE + 1,
            PANEL_COLUMN + 5,
            row_of(&panel, 0)
        )),
        "the panel did not move with the box: {output:?}"
    );
}

#[test]
fn a_panel_a_screen_refused_is_drawn_again() {
    let screen = Switchable::new();
    let mut viewport = Viewport::new(screen.clone());
    viewport
        .render(&Frame::blank())
        .expect("the screen is accepting writes");
    viewport
        .show_panel(&Legend::blank().picture(&ScriptedControls::new([])))
        .expect("the screen is accepting writes");

    screen.refuse(true);
    let failed = viewport.show_panel(&picture());

    screen.refuse(false);
    let already_written = screen.len();
    let recovered = viewport.show_panel(&picture());

    assert_eq!(failed, Err(RenderError::WriteFailed));
    assert_eq!(recovered, Ok(()));
    assert!(
        screen
            .since(already_written)
            .contains(&row_of(&picture(), 0)),
        "the panel was not drawn again after the write that failed"
    );
}

#[test]
fn the_panel_a_screen_last_took_is_drawn_again_after_one_it_refused() {
    let screen = Switchable::new();
    let mut viewport = Viewport::new(screen.clone());
    viewport
        .render(&Frame::blank())
        .expect("the screen is accepting writes");
    viewport
        .show_panel(&picture())
        .expect("the screen is accepting writes");

    screen.refuse(true);
    let failed = viewport.show_panel(&Legend::blank().picture(&ScriptedControls::new([])));

    screen.refuse(false);
    let already_written = screen.len();
    viewport
        .show_panel(&picture())
        .expect("the screen is accepting writes again");

    assert_eq!(failed, Err(RenderError::WriteFailed));
    assert!(
        screen
            .since(already_written)
            .contains(&row_of(&picture(), 0)),
        "a screen that refused a panel was taken to be showing the one before it"
    );
}

#[test]
fn the_panel_is_drawn_again_after_a_frame_failed_beside_it() {
    let screen = Switchable::new();
    let mut viewport = Viewport::new(screen.clone());
    viewport
        .render(&Frame::blank())
        .expect("the screen is accepting writes");
    viewport
        .show_panel(&picture())
        .expect("the screen is accepting writes");

    screen.refuse(true);
    let failed = viewport.render(&drawn(&[(3, 2, 'x')]));

    screen.refuse(false);
    let already_written = screen.len();
    viewport
        .render(&drawn(&[(3, 2, 'x')]))
        .expect("the screen is accepting writes again");
    viewport
        .show_panel(&picture())
        .expect("the screen is accepting writes again");

    assert_eq!(failed, Err(RenderError::WriteFailed));
    assert!(
        screen
            .since(already_written)
            .contains(&row_of(&picture(), 0)),
        "the panel was left for lost after a frame the screen refused"
    );
}

#[test]
fn a_moved_viewport_draws_the_frame_inside_its_new_border() {
    let mut viewport = Viewport::new(Vec::new());
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");

    viewport.place(5, 1);
    viewport
        .render(&Frame::blank())
        .expect("a vec accepts every write");
    let already_written = viewport.sink().len();
    viewport
        .render(&drawn(&[(0, 0, 'x')]))
        .expect("a vec accepts every write");

    let output = String::from_utf8(viewport.sink()[already_written..].to_vec())
        .expect("the output is utf-8");

    assert_eq!(output, "\u{1b}[3;7Hx");
}
