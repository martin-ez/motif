//! The shell that holds a page per mode: which one gets the controls, which one
//! draws, and what the shell keeps for itself.
//!
//! The pages here are the test's own, because which pages a shell holds is a
//! composition question and not something the shell knows. Each draws a glyph of
//! its own and keeps what it was handed, so a test reads back which page drew
//! and what reached it.

use std::cell::RefCell;
use std::rc::Rc;

use motif::device::Button;
use motif::ui::{
    App, Cell, ControlEvent, Controls, EventLoop, Flow, Frame, Legend, Mode, NullRenderer, Page,
    Region, Renderer, ScriptedClock, ScriptedControls, Shell,
};

const MARKER: char = '*';

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

/// What a page was handed, readable after the shell has taken it over.
#[derive(Clone, Default)]
struct Taken(Rc<RefCell<Vec<ControlEvent>>>);

impl Taken {
    fn events(&self) -> Vec<ControlEvent> {
        self.0.borrow().clone()
    }
}

struct Marked {
    glyph: char,
    answers: Button,
    taken: Taken,
}

impl Marked {
    fn new(glyph: char, answers: Button) -> Self {
        Self {
            glyph,
            answers,
            taken: Taken::default(),
        }
    }

    fn taken(&self) -> Taken {
        self.taken.clone()
    }
}

impl Page for Marked {
    fn control(&mut self, event: ControlEvent) {
        self.taken.0.borrow_mut().push(event);
    }

    fn legend(&self) -> Legend {
        Legend::blank().answering(self.answers)
    }

    fn draw(&mut self, mut region: Region<'_>) {
        region.set(0, 0, Cell::new(self.glyph));
    }
}

fn shell_of(page: Marked) -> Shell {
    Shell::new([Box::new(page)])
}

fn showing(glyph: char) -> (Shell, Taken) {
    let page = Marked::new(glyph, Button::Play);
    let taken = page.taken();

    (shell_of(page), taken)
}

fn driven_by(shell: &mut Shell, events: impl IntoIterator<Item = ControlEvent>) -> Flow {
    let mut controls = ScriptedControls::new(events);
    while let Some(event) = controls.poll() {
        if shell.control(event).is_exit() {
            return Flow::Exit;
        }
    }

    Flow::Continue
}

fn drawn(shell: &mut Shell) -> Frame {
    let mut frame = Frame::blank();
    shell.draw(frame.region());

    frame
}

#[test]
fn a_shell_opens_on_the_first_mode() {
    let (shell, _) = showing(MARKER);

    assert_eq!(shell.showing(), Mode::ALL[0]);
}

#[test]
fn the_showing_page_draws_the_frame() {
    let (mut shell, _) = showing(MARKER);

    assert_eq!(drawn(&mut shell).get(0, 0), Some(Cell::new(MARKER)));
}

#[test]
fn a_control_reaches_the_showing_page() {
    let (mut shell, taken) = showing(MARKER);

    driven_by(&mut shell, [pressed(Button::Play)]);

    assert_eq!(taken.events(), vec![pressed(Button::Play)]);
}

#[test]
fn the_shell_takes_its_legend_from_the_showing_page() {
    let page = Marked::new(MARKER, Button::Record);
    let shell = shell_of(page);

    assert!(shell.legend().answers(Button::Record));
}

#[test]
fn the_shell_declares_the_shift_it_keeps() {
    let shell = shell_of(Marked::new(MARKER, Button::Play));

    assert!(shell.legend().answers(Button::Shift));
}

#[test]
fn the_shell_declares_the_stop_it_keeps() {
    let shell = shell_of(Marked::new(MARKER, Button::Play));

    assert!(shell.legend().answers(Button::Stop));
}

#[test]
fn shift_and_stop_ends_the_run() {
    let (mut shell, _) = showing(MARKER);

    assert_eq!(shell.control(shifted(Button::Stop)), Flow::Exit);
}

#[test]
fn the_control_the_shell_keeps_does_not_reach_the_page() {
    let (mut shell, taken) = showing(MARKER);

    driven_by(&mut shell, [shifted(Button::Stop)]);

    assert!(taken.events().is_empty());
}

#[test]
fn stop_on_its_own_reaches_the_page() {
    let (mut shell, taken) = showing(MARKER);

    driven_by(&mut shell, [pressed(Button::Stop)]);

    assert_eq!(taken.events(), vec![pressed(Button::Stop)]);
}

#[test]
fn a_shifted_control_that_is_not_stop_keeps_the_run_going() {
    let (mut shell, _) = showing(MARKER);

    for button in Button::ALL {
        if matches!(button, Button::Stop) {
            continue;
        }
        assert_eq!(shell.control(shifted(button)), Flow::Continue);
    }
}

#[test]
fn a_shifted_control_that_is_not_stop_reaches_the_page() {
    let (mut shell, taken) = showing(MARKER);

    driven_by(&mut shell, [shifted(Button::Play)]);

    assert_eq!(taken.events(), vec![shifted(Button::Play)]);
}

#[test]
fn a_control_a_page_answers_does_not_end_the_run() {
    let (mut shell, _) = showing(MARKER);

    assert_eq!(shell.control(pressed(Button::Play)), Flow::Continue);
}

#[test]
fn a_draw_does_not_end_the_run() {
    let (mut shell, _) = showing(MARKER);

    assert_eq!(shell.draw(Frame::blank().region()), Flow::Continue);
}

#[test]
fn a_shell_driven_by_controls_renders_the_page_that_drew() {
    let (mut shell, _) = showing(MARKER);
    let mut screen = NullRenderer::new();

    driven_by(&mut shell, [pressed(Button::Play)]);
    let frame = drawn(&mut shell);
    screen.render(&frame).expect("the null renderer takes it");

    assert_eq!(
        screen.rendered().and_then(|frame| frame.get(0, 0)),
        Some(Cell::new(MARKER))
    );
}

#[test]
fn the_event_loop_runs_a_shell_until_it_is_asked_to_stop() {
    let (mut shell, _) = showing(MARKER);
    let mut controls = ScriptedControls::new([shifted(Button::Stop)]);
    let mut screen = NullRenderer::new();

    let report = EventLoop::with_clock(ScriptedClock::new([]))
        .run(&mut shell, &mut controls, &mut screen)
        .expect("the null renderer takes every frame");

    assert_eq!(report.frames(), 0);
}
