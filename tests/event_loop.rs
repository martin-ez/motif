//! The loop that ties controls, application state and drawing together.
//!
//! Nothing here names a terminal or a key: the loop is handed a panel and a
//! screen, and an application that knows only about controls and frames.

use std::time::{Duration, Instant};

use motif::device::{Button, DeviceProfile, Encoder};
use motif::ui::{
    App, Cell, ControlEvent, Controls, EVENTS_PER_FRAME, EventLoop, Flow, Frame, NullRenderer,
    Panel, Region, RenderError, Renderer, ScriptedClock, ScriptedControls, Turn,
};

/// The edge of a key, which is drawn for every control whether the page answers
/// it or not, so a picture of the panel can be told from anything else drawn.
const KEY_EDGE: char = '┌';

/// The edge of a key whose event has just been delivered, drawn nowhere on a
/// panel every control of which is at rest.
const MARKED_EDGE: char = '┃';

/// How many frames that mark lasts, written out rather than read from the crate
/// so that a change to the decay fails a test instead of retuning the frames
/// these walk.
const MARKED_FRAMES: usize = 3;

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

/// A panel with a way out of its own, taken once it has been polled the stated
/// number of times. Standing in for the key a terminal ends a run with, which
/// no application declares and none can refuse.
struct Escapable {
    polls_first: usize,
}

impl Escapable {
    fn after(polls: usize) -> Self {
        Self { polls_first: polls }
    }
}

impl Controls for Escapable {
    fn poll(&mut self) -> Option<ControlEvent> {
        self.polls_first = self.polls_first.saturating_sub(1);
        None
    }

    fn interrupted(&self) -> bool {
        self.polls_first == 0
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

/// A screen keeping every panel it is shown rather than only the last, so a
/// test can watch a mark arrive and settle instead of seeing where it ended up.
struct Recording {
    panels: Vec<Panel>,
    accepts: usize,
}

impl Recording {
    fn accepting(frames: usize) -> Self {
        Self {
            panels: Vec::new(),
            accepts: frames,
        }
    }

    /// Whether anything on the panel shown on `frame` is marked, counting from
    /// zero at the first frame of the run.
    fn marked_on(&self, frame: usize) -> bool {
        let shown = self.panels.get(frame).expect("the run drew this frame");

        picture_of(shown).contains(MARKED_EDGE)
    }
}

impl Renderer for Recording {
    fn render(&mut self, _frame: &Frame) -> Result<(), RenderError> {
        self.accepts = self
            .accepts
            .checked_sub(1)
            .ok_or(RenderError::Unavailable)?;

        Ok(())
    }

    fn show_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        self.panels.push(panel.clone());
        Ok(())
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
fn a_run_ends_when_the_panel_is_interrupted() {
    let mut app = Page::lasting(usize::MAX);
    let mut controls = Escapable::after(0);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let report = still().run(&mut app, &mut controls, &mut screen);

    assert!(report.is_ok());
}

#[test]
fn an_interrupted_panel_ends_a_run_the_application_would_not() {
    let mut app = Page::lasting(usize::MAX);
    let mut controls = Escapable::after(2);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    let report = still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.frames(), 1);
}

#[test]
fn an_interrupted_panel_leaves_the_frame_it_ended_undrawn() {
    let mut app = Page::lasting(usize::MAX);
    let mut controls = Escapable::after(0);
    let mut screen = Patient::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(app.drawn, 0);
    assert_eq!(screen.rendered(), None);
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
/// panel drawn into the frame would take first.
struct Filling;

impl App for Filling {
    fn control(&mut self, _event: ControlEvent) -> Flow {
        Flow::Continue
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

fn recorded(events: impl IntoIterator<Item = ControlEvent>, frames: usize) -> Recording {
    let mut app = Page::lasting(frames);
    let mut controls = ScriptedControls::new(events);
    let mut screen = Recording::accepting(FRAMES_ACCEPTED);

    still()
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    screen
}

#[test]
fn a_control_marks_its_key_on_the_frame_its_event_is_delivered() {
    let screen = recorded([pressed(Button::Play)], MARKED_FRAMES + 2);

    assert!(screen.marked_on(0));
}

#[test]
fn a_mark_stays_up_for_every_frame_of_the_decay() {
    let screen = recorded([pressed(Button::Play)], MARKED_FRAMES + 2);

    for frame in 0..MARKED_FRAMES {
        assert!(screen.marked_on(frame), "settled on frame {frame}");
    }
}

#[test]
fn a_mark_settles_back_the_stated_number_of_frames_later() {
    let screen = recorded([pressed(Button::Play)], MARKED_FRAMES + 2);

    assert!(!screen.marked_on(MARKED_FRAMES));
}

#[test]
fn a_control_the_page_does_not_answer_marks_all_the_same() {
    let screen = recorded([pressed(Button::Record)], MARKED_FRAMES + 2);

    assert!(screen.marked_on(0));
}

#[test]
fn an_encoder_turned_marks_the_panel_as_a_press_does() {
    let screen = recorded(
        [ControlEvent::Turned {
            encoder: Encoder::Main,
            turn: Turn::Clockwise,
            shifted: false,
        }],
        MARKED_FRAMES + 2,
    );

    assert!(screen.marked_on(0));
    assert!(!screen.marked_on(MARKED_FRAMES));
}

#[test]
fn a_run_nobody_touches_marks_nothing() {
    let screen = recorded([], MARKED_FRAMES + 2);

    for frame in 0..MARKED_FRAMES {
        assert!(!screen.marked_on(frame), "marked on frame {frame}");
    }
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
