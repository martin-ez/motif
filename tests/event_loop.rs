//! The loop that ties controls, application state and drawing together.
//!
//! Nothing here names a terminal or a key: the loop is handed a panel and a
//! screen, and an application that knows only about controls and frames.

use std::time::{Duration, Instant};

use motif::device::{Button, DeviceProfile};
use motif::ui::{
    App, Cell, ControlEvent, Controls, EVENTS_PER_FRAME, EventLoop, Flow, Frame, Legend,
    NullRenderer, Panel, Region, RenderError, Renderer, ScriptedClock, ScriptedControls,
};

/// The edge of a key, which is drawn for every control whether the page answers
/// it or not, so a picture of the panel can be told from anything else drawn.
const KEY_EDGE: char = '┌';

const BUDGET: Duration = DeviceProfile::TARGET.screen.frame_budget();

/// More frames than any run here draws, so a loop that will not stop fails an
/// assertion rather than running until the harness gives up on it.
const FRAMES_ACCEPTED: usize = 8;

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

/// An application that marks each frame it draws on a row of its own.
struct Page {
    seen: Vec<ControlEvent>,
    drawn: usize,
    draws_before_exit: usize,
    quits_on: Option<Button>,
}

impl Page {
    fn lasting(draws: usize) -> Self {
        Self {
            seen: Vec::new(),
            drawn: 0,
            draws_before_exit: draws,
            quits_on: None,
        }
    }

    fn quitting_on(button: Button) -> Self {
        Self {
            quits_on: Some(button),
            ..Self::lasting(usize::MAX)
        }
    }
}

impl App for Page {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.seen.push(event);

        match event {
            ControlEvent::Pressed { button, .. } if self.quits_on == Some(button) => Flow::Exit,
            _ => Flow::Continue,
        }
    }

    fn legend(&self) -> Legend {
        Legend::blank().answering(Button::Play)
    }

    fn draw(&mut self, mut region: Region<'_>) -> Flow {
        region.set(0, self.drawn, Cell::new('*'));
        self.drawn += 1;

        if self.drawn >= self.draws_before_exit {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

/// A panel that always has one more event, as a terminal handed a pasted page
/// of text does.
struct Endless(ControlEvent);

impl Controls for Endless {
    fn poll(&mut self) -> Option<ControlEvent> {
        Some(self.0)
    }
}

/// A screen that takes a stated number of frames and then reports failure.
///
/// A loop that will not stop makes a test that never ends, and a test that never
/// ends reports nothing. Bounding what the screen will accept turns that into an
/// assertion failing at once.
struct Patient {
    screen: NullRenderer,
    accepts: usize,
}

impl Patient {
    fn accepting(frames: usize) -> Self {
        Self {
            screen: NullRenderer::new(),
            accepts: frames,
        }
    }

    fn rendered(&self) -> Option<&Frame> {
        self.screen.rendered()
    }

    fn shown(&self) -> Option<&Panel> {
        self.screen.shown()
    }
}

impl Renderer for Patient {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        self.accepts = self
            .accepts
            .checked_sub(1)
            .ok_or(RenderError::Unavailable)?;

        self.screen.render(frame)
    }

    fn show_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        self.screen.show_panel(panel)
    }
}

struct BrokenScreen;

impl Renderer for BrokenScreen {
    fn render(&mut self, _frame: &Frame) -> Result<(), RenderError> {
        Err(RenderError::WriteFailed)
    }
}

fn scripted(readings: impl IntoIterator<Item = Duration>) -> EventLoop<ScriptedClock> {
    EventLoop::with_clock(ScriptedClock::new(readings))
}

fn still() -> EventLoop<ScriptedClock> {
    scripted([])
}

#[test]
fn a_run_ends_when_the_application_asks_to_exit() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let report = still().run(&mut app, &mut controls, &mut screen);

    assert_eq!(report.map(|report| report.frames()), Ok(1));
}

#[test]
fn a_run_ends_when_a_control_asks_to_exit() {
    let mut app = Page::quitting_on(Button::Stop);
    let mut controls = ScriptedControls::new([pressed(Button::Stop)]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let report = still().run(&mut app, &mut controls, &mut screen);

    assert!(report.is_ok());
}

#[test]
fn a_control_that_ends_the_run_is_not_drawn_after() {
    let mut app = Page::quitting_on(Button::Stop);
    let mut controls = ScriptedControls::new([pressed(Button::Stop)]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(app.drawn, 0);
    assert_eq!(screen.rendered(), None);
}

#[test]
fn events_waiting_behind_an_exit_are_left_unread() {
    let mut app = Page::quitting_on(Button::Stop);
    let mut controls = ScriptedControls::new([pressed(Button::Stop), pressed(Button::Play)]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(controls.poll(), Some(pressed(Button::Play)));
}

#[test]
fn every_event_waiting_reaches_the_application_in_one_frame() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([pressed(Button::Play), pressed(Button::Record)]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(app.seen, [pressed(Button::Play), pressed(Button::Record)]);
}

#[test]
fn a_frame_takes_no_more_events_than_the_bound_allows() {
    let mut app = Page::lasting(1);
    let mut controls = Endless(pressed(Button::Play));
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(app.seen.len(), EVENTS_PER_FRAME);
}

#[test]
fn a_panel_that_never_runs_dry_still_reaches_the_screen() {
    let mut app = Page::lasting(1);
    let mut controls = Endless(pressed(Button::Play));
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let report = still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.frames(), 1);
}

#[test]
fn events_past_the_bound_wait_for_the_next_frame() {
    let mut app = Page::lasting(1);
    let waiting = std::iter::repeat_n(pressed(Button::Play), EVENTS_PER_FRAME + 1);
    let mut controls = ScriptedControls::new(waiting);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(controls.poll(), Some(pressed(Button::Play)));
}

#[test]
fn what_the_application_drew_is_what_the_screen_is_given() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    let drawn = screen.rendered().and_then(|frame| frame.get(0, 0));
    assert_eq!(drawn, Some(Cell::new('*')));
}

#[test]
fn each_frame_starts_blank() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    let rendered = screen.rendered().expect("two frames were drawn");
    assert_eq!(rendered.get(0, 0), Some(Cell::BLANK));
    assert_eq!(rendered.get(0, 1), Some(Cell::new('*')));
}

#[test]
fn a_frame_is_counted_for_every_time_the_screen_was_drawn() {
    let mut app = Page::lasting(3);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let report = still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.frames(), 3);
}

#[test]
fn a_frame_with_time_to_spare_gives_the_rest_of_it_back() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);
    let mut loops = scripted([
        Duration::ZERO,
        Duration::from_millis(3),
        Duration::from_millis(3),
        Duration::from_millis(6),
    ]);

    loops
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(loops.clock().slept(), [BUDGET - Duration::from_millis(3)]);
}

#[test]
fn a_frame_that_ran_over_its_budget_does_not_wait() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);
    let mut loops = scripted([Duration::ZERO, BUDGET + Duration::from_millis(1)]);

    loops
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert!(loops.clock().slept().is_empty());
}

#[test]
fn a_frame_that_ran_over_its_budget_is_counted_as_an_overrun() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);
    let mut loops = scripted([Duration::ZERO, BUDGET + Duration::from_millis(1)]);

    let report = loops
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.overruns(), 1);
}

#[test]
fn a_frame_inside_its_budget_is_no_overrun() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);
    let mut loops = scripted([Duration::ZERO, Duration::from_millis(1)]);

    let report = loops
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.overruns(), 0);
}

#[test]
fn the_last_frame_of_a_run_is_not_paced() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);
    let mut loops = scripted([Duration::ZERO, Duration::from_millis(1)]);

    loops
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert!(loops.clock().slept().is_empty());
}

#[test]
fn the_last_frame_of_a_run_is_counted_when_it_runs_over() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);
    let mut loops = scripted([Duration::ZERO, BUDGET + Duration::from_millis(1)]);

    let report = loops
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.overruns(), 1);
    assert!(loops.clock().slept().is_empty());
}

#[test]
fn a_screen_that_cannot_be_written_ends_the_run() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = BrokenScreen;

    let report = still().run(&mut app, &mut controls, &mut screen);

    assert_eq!(report.err(), Some(RenderError::WriteFailed));
}

#[test]
fn a_screen_that_cannot_be_written_is_not_drawn_again() {
    let mut app = Page::lasting(usize::MAX);
    let mut controls = ScriptedControls::new([]);
    let mut screen = BrokenScreen;

    let failed = still().run(&mut app, &mut controls, &mut screen);

    assert!(failed.is_err());
    assert_eq!(app.drawn, 1);
}

/// An application that draws in the last row of the screen, which is the row a
/// legend drawn into the frame would take first.
struct Filling;

impl App for Filling {
    fn control(&mut self, _event: ControlEvent) -> Flow {
        Flow::Continue
    }

    fn legend(&self) -> Legend {
        Legend::blank().answering(Button::Play)
    }

    fn draw(&mut self, mut region: Region<'_>) -> Flow {
        for column in 0..region.columns() {
            region.set(column, region.rows() - 1, Cell::new('#'));
        }

        Flow::Exit
    }
}

fn text_of(frame: &Frame) -> String {
    let screen = DeviceProfile::TARGET.screen;
    let mut text = String::new();

    for row in 0..screen.rows {
        for column in 0..screen.columns {
            text.push(frame.get(column, row).unwrap_or(Cell::BLANK).glyph());
        }
        text.push('\n');
    }

    text
}

fn picture_of(panel: &Panel) -> String {
    let mut text = String::new();

    for row in 0..Panel::ROWS {
        for column in 0..Panel::COLUMNS {
            text.push(panel.get(column, row).unwrap_or(Cell::BLANK).glyph());
        }
        text.push('\n');
    }

    text
}

#[test]
fn a_run_hands_the_screen_what_the_page_declares() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    let shown = screen.shown().expect("the panel was shown");
    assert!(picture_of(shown).contains(KEY_EDGE));
}

#[test]
fn the_panel_is_shown_beside_the_frame_rather_than_in_it() {
    let mut app = Page::lasting(1);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    let drawn = screen.rendered().expect("a frame was drawn");
    assert!(!text_of(drawn).contains(KEY_EDGE));
}

#[test]
fn the_last_row_of_the_screen_is_the_page_s_to_draw_in() {
    let mut app = Filling;
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    let drawn = screen.rendered().expect("a frame was drawn");
    assert!(text_of(drawn).contains('#'));
}

#[test]
fn a_run_that_draws_nothing_shows_no_panel() {
    let mut app = Page::quitting_on(Button::Stop);
    let mut controls = ScriptedControls::new([pressed(Button::Stop)]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(screen.shown(), None);
}

#[test]
fn a_run_on_the_machine_clock_spends_a_budget_on_every_frame_but_the_last() {
    let mut app = Page::lasting(2);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let started = Instant::now();
    EventLoop::new()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert!(started.elapsed() >= BUDGET);
}
