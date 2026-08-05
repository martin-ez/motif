//! How much of a frame budget the event loop spent, measured as it runs and
//! read while it is still running.
//!
//! The facts worth stating are the fraction itself — a frame's work against the
//! budget it had — that a spike survives long enough for a person to see it,
//! that the overrun count is readable before the run ends and agrees with the
//! one the report gives afterwards, and that a draw reads the frame before it
//! rather than the one it is in.
//!
//! Frame costs are stated rather than spent: the loop runs on a
//! [`ScriptedClock`], so a frame that took half its budget costs the test
//! nothing.

#![cfg(feature = "frame-pace")]

use std::time::Duration;

use motif::device::{Button, DeviceProfile};
use motif::ui::{
    App, Cell, ControlEvent, EventLoop, Flow, Frame, Legend, Pace, PaceReader, RenderError,
    Renderer, ScriptedClock, ScriptedControls, pace_meter,
};

const BUDGET: Duration = DeviceProfile::TARGET.screen.frame_budget();

/// A second of frames at the target's 30 Hz, stated rather than taken from
/// [`Pace::RECENT_FRAMES`]: a window sized from the constant it is meant to
/// pin down moves whenever the constant does, and pins down nothing.
const RECENT_FRAMES: usize = 30;

/// Frames enough to close the recent window twice over, so that whatever the
/// window holds has been replaced rather than merely added to.
const PAST_THE_WINDOW: usize = 128;

/// More frames than any run here draws, so a loop that will not stop fails an
/// assertion rather than running until the harness gives up on it.
const FRAMES_ACCEPTED: usize = 8;

/// The budget is 33 333 333 ns, so a fraction of it is not a whole number of
/// nanoseconds and a frame costing "half the budget" is a nanosecond short of
/// one. That is the truncation being allowed for, not a measurement error.
fn assert_near(measured: f32, expected: f32) {
    assert!(
        (measured - expected).abs() < 1e-6,
        "{measured} is not {expected}"
    );
}

fn quiet_frames(writer: &mut motif::ui::PaceWriter, frames: usize) {
    for _ in 0..frames {
        writer.measured(Duration::ZERO, 0);
    }
}

#[test]
fn a_meter_starts_idle() {
    let (_writer, reader) = pace_meter();

    assert_eq!(reader.read(), Pace::IDLE);
}

#[test]
fn a_frame_that_used_half_the_budget_reads_half() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET / 2, 0);

    assert_near(reader.read().load, 0.5);
}

#[test]
fn a_frame_that_used_all_of_the_budget_reads_one() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET, 0);

    assert_eq!(reader.read().load, 1.0);
}

#[test]
fn a_frame_that_overran_its_budget_reads_above_one() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET * 2, 1);

    assert_eq!(reader.read().load, 2.0);
}

#[test]
fn the_load_read_is_the_last_frame_measured() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET, 0);
    writer.measured(BUDGET / 4, 0);

    assert_near(reader.read().load, 0.25);
}

#[test]
fn measuring_reports_what_it_published() {
    let (mut writer, reader) = pace_meter();

    let measured = writer.measured(BUDGET / 2, 0);

    assert_eq!(measured, reader.read());
}

#[test]
fn reading_does_not_reset_what_was_measured() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET / 2, 0);

    assert_eq!(reader.read(), reader.read());
}

#[test]
fn the_overruns_read_are_the_ones_it_was_given() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET / 2, 7);

    assert_eq!(reader.read().overruns, 7);
}

#[test]
fn the_peak_holds_a_spike_a_later_frame_did_not_repeat() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET, 0);
    writer.measured(Duration::ZERO, 0);

    assert_eq!(reader.read().peak, 1.0);
}

#[test]
fn a_spike_survives_the_window_it_landed_in_closing() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET, 0);
    quiet_frames(&mut writer, RECENT_FRAMES);

    assert_eq!(reader.read().peak, 1.0);
}

#[test]
fn the_recent_window_spans_a_second_of_frames() {
    assert_eq!(Pace::RECENT_FRAMES, RECENT_FRAMES);
}

#[test]
fn the_peak_falls_back_once_the_spike_has_left_the_window() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET, 0);
    quiet_frames(&mut writer, PAST_THE_WINDOW);

    assert_eq!(reader.read().peak, 0.0);
}

#[test]
fn the_load_never_reads_above_the_peak_it_arrives_with() {
    let (mut writer, reader) = pace_meter();

    for frame in 0..PAST_THE_WINDOW {
        writer.measured(BUDGET * (frame as u32 % 7), 0);
        let read = reader.read();

        assert!(read.load <= read.peak);
    }
}

#[test]
fn spare_is_what_the_worst_recent_frame_left_unused() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET / 4, 0);

    assert_eq!(reader.read().spare(), 0.75);
}

#[test]
fn spare_is_negative_where_a_frame_overran() {
    let (mut writer, reader) = pace_meter();

    writer.measured(BUDGET * 2, 1);

    assert_eq!(reader.read().spare(), -1.0);
}

#[test]
fn an_idle_loop_has_all_of_its_budget_spare() {
    assert_eq!(Pace::IDLE.spare(), 1.0);
}

/// An application that records what the meter read at each of its draws, and
/// stops after a stated number of them.
struct Watching {
    reader: PaceReader,
    read: Vec<Pace>,
    draws_before_exit: usize,
}

impl Watching {
    fn lasting(draws: usize, reader: PaceReader) -> Self {
        Self {
            reader,
            read: Vec::new(),
            draws_before_exit: draws,
        }
    }
}

impl App for Watching {
    fn control(&mut self, _event: ControlEvent) -> Flow {
        Flow::Continue
    }

    fn legend(&self) -> Legend {
        Legend::blank().answering(Button::Play)
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        self.read.push(self.reader.read());
        frame.set(0, 0, Cell::new('*'));

        if self.read.len() >= self.draws_before_exit {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

/// A screen that takes a stated number of frames and then reports failure, so
/// that a loop which will not stop fails rather than hangs.
struct Patient(usize);

impl Renderer for Patient {
    fn render(&mut self, _frame: &Frame) -> Result<(), RenderError> {
        self.0 = self.0.checked_sub(1).ok_or(RenderError::Unavailable)?;

        Ok(())
    }
}

/// The clock readings a run of `costs` frames observes, each frame starting
/// where the one before it ended.
fn readings(costs: impl IntoIterator<Item = Duration>) -> Vec<Duration> {
    let mut readings = Vec::new();
    let mut elapsed = Duration::ZERO;

    for cost in costs {
        readings.push(elapsed);
        elapsed += cost;
        readings.push(elapsed);
    }

    readings
}

#[test]
fn the_first_frame_is_drawn_before_anything_is_measured() {
    let (writer, reader) = pace_meter();
    let mut app = Watching::lasting(1, reader);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient(FRAMES_ACCEPTED);

    EventLoop::with_clock(ScriptedClock::new(readings([BUDGET / 2])))
        .metering(writer)
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(app.read, [Pace::IDLE]);
}

#[test]
fn a_frame_is_measured_before_the_next_one_is_drawn() {
    let (writer, reader) = pace_meter();
    let mut app = Watching::lasting(2, reader);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient(FRAMES_ACCEPTED);

    EventLoop::with_clock(ScriptedClock::new(readings([BUDGET / 2, BUDGET / 4])))
        .metering(writer)
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_near(app.read[1].load, 0.5);
}

#[test]
fn the_pace_a_draw_reads_is_the_frame_before_it() {
    let (writer, reader) = pace_meter();
    let mut app = Watching::lasting(3, reader);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient(FRAMES_ACCEPTED);

    EventLoop::with_clock(ScriptedClock::new(readings([
        BUDGET / 2,
        BUDGET / 4,
        BUDGET,
    ])))
    .metering(writer)
    .run(&mut app, &mut controls, &mut screen)
    .expect("the screen accepts the frames this run draws");

    assert_near(app.read[0].load, 0.0);
    assert_near(app.read[1].load, 0.5);
    assert_near(app.read[2].load, 0.25);
}

#[test]
fn an_overrun_can_be_read_before_the_run_ends() {
    let (writer, reader) = pace_meter();
    let mut app = Watching::lasting(2, reader);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient(FRAMES_ACCEPTED);

    EventLoop::with_clock(ScriptedClock::new(readings([
        BUDGET + Duration::from_millis(1),
        BUDGET / 4,
    ])))
    .metering(writer)
    .run(&mut app, &mut controls, &mut screen)
    .expect("the screen accepts the frames this run draws");

    assert_eq!(app.read[1].overruns, 1);
}

#[test]
fn the_overruns_read_during_a_run_match_the_report() {
    let (writer, reader) = pace_meter();
    let mut app = Watching::lasting(3, reader);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient(FRAMES_ACCEPTED);

    let report = EventLoop::with_clock(ScriptedClock::new(readings([
        BUDGET * 2,
        BUDGET * 2,
        BUDGET / 4,
    ])))
    .metering(writer)
    .run(&mut app, &mut controls, &mut screen)
    .expect("the screen accepts the frames this run draws");

    assert_eq!(app.read[2].overruns, 2);
    assert_eq!(report.overruns(), 2);
}

#[test]
fn a_loop_with_no_meter_draws_the_same_frames() {
    let (_writer, reader) = pace_meter();
    let mut app = Watching::lasting(2, reader);
    let mut controls = ScriptedControls::new([]);
    let mut screen = Patient(FRAMES_ACCEPTED);

    let report = EventLoop::with_clock(ScriptedClock::new(readings([BUDGET / 2, BUDGET / 4])))
        .run(&mut app, &mut controls, &mut screen)
        .expect("the screen accepts the frames this run draws");

    assert_eq!(report.frames(), 2);
    assert_eq!(app.read, [Pace::IDLE, Pace::IDLE]);
}
