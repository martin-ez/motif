//! The row the application never sees: what the chrome draws on it, and that
//! everything else about the application passes through it untouched.
//!
//! The application under it is the test's own, and it fills every row it was
//! handed. One that keeps all of its region is what makes the split visible: the
//! rows it reached are the rows it was given, and the one the chrome took carries
//! the name instead.

use std::cell::RefCell;
use std::rc::Rc;

use motif::device::Button;
use motif::ui::{App, Cell, Chrome, ControlEvent, Flow, Frame, Region};

const MARKER: char = '*';

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

/// What the application was handed, readable after the chrome has taken it over.
#[derive(Clone, Default)]
struct Taken(Rc<RefCell<Vec<ControlEvent>>>);

impl Taken {
    fn events(&self) -> Vec<ControlEvent> {
        self.0.borrow().clone()
    }
}

/// An application filling every row it is handed, keeping what reached it.
struct Filling {
    taken: Taken,
    flow: Flow,
}

impl Filling {
    fn new() -> Self {
        Self {
            taken: Taken::default(),
            flow: Flow::Continue,
        }
    }

    fn ending() -> Self {
        Self {
            flow: Flow::Exit,
            ..Self::new()
        }
    }

    fn taken(&self) -> Taken {
        self.taken.clone()
    }
}

impl App for Filling {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.taken.0.borrow_mut().push(event);

        self.flow
    }

    fn draw(&mut self, mut region: Region<'_>) -> Flow {
        for row in 0..region.rows() {
            region.set(0, row, Cell::new(MARKER));
        }

        self.flow
    }
}

fn around(app: Filling) -> (Chrome<Filling>, Taken) {
    let taken = app.taken();

    (Chrome::around(app), taken)
}

fn drawn(chrome: &mut Chrome<Filling>) -> Frame {
    let mut frame = Frame::blank();
    chrome.draw(frame.region());

    frame
}

fn row(frame: &Frame, row: usize) -> String {
    (0..)
        .map_while(|column| frame.get(column, row))
        .map(Cell::glyph)
        .collect()
}

fn rows(frame: &Frame) -> usize {
    (0..).take_while(|&row| frame.get(0, row).is_some()).count()
}

fn filled(frame: &Frame) -> Vec<usize> {
    (0..rows(frame))
        .filter(|&row| frame.get(0, row) == Some(Cell::new(MARKER)))
        .collect()
}

#[test]
fn the_instrument_is_named_on_the_top_row() {
    let (mut chrome, _) = around(Filling::new());

    let frame = drawn(&mut chrome);

    assert!(row(&frame, 0).contains("motif"));
}

#[test]
fn the_name_is_drawn_against_the_right_margin() {
    let (mut chrome, _) = around(Filling::new());

    let frame = drawn(&mut chrome);
    let top = row(&frame, 0);

    assert_eq!(top.trim_end(), top);
    assert!(top.starts_with(' '));
}

#[test]
fn the_application_keeps_every_row_below() {
    let (mut chrome, _) = around(Filling::new());

    let frame = drawn(&mut chrome);
    let below: Vec<usize> = (1..rows(&frame)).collect();

    assert_eq!(filled(&frame), below);
}

#[test]
fn the_row_the_chrome_took_is_not_the_applications() {
    let (mut chrome, _) = around(Filling::new());

    let frame = drawn(&mut chrome);

    assert_ne!(frame.get(0, 0), Some(Cell::new(MARKER)));
}

#[test]
fn nothing_is_drawn_below_the_application() {
    let (mut chrome, _) = around(Filling::new());

    let frame = drawn(&mut chrome);
    let last = rows(&frame) - 1;

    assert_eq!(frame.get(0, last), Some(Cell::new(MARKER)));
}

#[test]
fn a_control_reaches_the_application() {
    let (mut chrome, taken) = around(Filling::new());

    chrome.control(pressed(Button::Play));

    assert_eq!(taken.events(), vec![pressed(Button::Play)]);
}

#[test]
fn the_application_says_when_a_control_ends_the_run() {
    let (mut chrome, _) = around(Filling::ending());

    assert_eq!(chrome.control(pressed(Button::Stop)), Flow::Exit);
}

#[test]
fn the_application_says_when_a_frame_ends_the_run() {
    let (mut chrome, _) = around(Filling::ending());

    assert_eq!(chrome.draw(Frame::blank().region()), Flow::Exit);
}

#[test]
fn a_frame_the_application_carries_on_from_keeps_the_run_going() {
    let (mut chrome, _) = around(Filling::new());

    assert_eq!(chrome.draw(Frame::blank().region()), Flow::Continue);
}
