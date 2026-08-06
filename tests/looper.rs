//! The looper as a player meets it: a page they press, and the engine those
//! presses reach.
//!
//! The two halves are wired by the composition rather than by either of them,
//! and this is where that wiring is stated: an order crosses the queue the pair
//! shares, and the playhead comes back over the meter they share. Nothing here
//! opens a device — blocks are rendered by hand, which is what the callback
//! would do and what a test can watch.
//!
//! The readout is read off the frame the page drew, because that is what a
//! player has. A page reading a meter nobody publishes to draws the same screen
//! as a loop that is empty, which is the whole of what this file is for.

use motif::audio::{AudioPath, Commanded, sample_clock};
use motif::device::{AudioProfile, Button, DeviceProfile, ScreenProfile};
use motif::looper::{LoopEngine, LooperPage};
use motif::ui::{ControlEvent, Frame, Page};

const AUDIO: AudioProfile = DeviceProfile::TARGET.audio;
const SCREEN: ScreenProfile = DeviceProfile::TARGET.screen;
const SECOND: usize = AUDIO.sample_rate as usize;
const INPUT: f32 = 0.5;

fn pressed(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: false,
    }
}

fn looper() -> (LooperPage, Commanded<LoopEngine>) {
    let (page, engine, _takes) = LooperPage::driving(AUDIO, sample_clock(AUDIO.sample_rate).1);

    (page, engine)
}

fn rendering(engine: &mut Commanded<LoopEngine>, frames: usize) {
    engine.render(&vec![INPUT; frames], &mut vec![0.0; frames]);
}

fn drawn(page: &mut LooperPage) -> Vec<String> {
    let mut frame = Frame::blank();
    page.draw(frame.region());

    (0..SCREEN.rows)
        .map(|row| {
            (0..SCREEN.columns)
                .filter_map(|column| frame.get(column, row))
                .map(|cell| cell.glyph())
                .collect::<String>()
        })
        .collect()
}

fn screen_of(page: &mut LooperPage) -> String {
    drawn(page).join("\n")
}

fn bar_of(page: &mut LooperPage) -> String {
    drawn(page)
        .iter()
        .map(|row| row.trim_end().to_owned())
        .find(|row| row.starts_with('['))
        .expect("the page draws a loop bar")
}

fn filled(bar: &str) -> usize {
    bar.chars().filter(|glyph| *glyph == '#').count()
}

fn recorded(seconds: usize) -> (LooperPage, Commanded<LoopEngine>) {
    let (mut page, mut engine) = looper();
    page.control(pressed(Button::Record));
    rendering(&mut engine, seconds * SECOND);

    (page, engine)
}

#[test]
fn a_looper_nobody_has_played_reads_an_empty_loop() {
    let (mut page, _engine) = looper();

    assert!(screen_of(&mut page).contains("0:00.0 / 0:00.0"));
}

#[test]
fn the_page_reads_the_playhead_the_engine_publishes() {
    let (mut page, _engine) = recorded(1);

    assert!(screen_of(&mut page).contains("0:01.0 / 0:01.0"));
}

#[test]
fn the_readout_follows_the_playhead_around_the_loop() {
    let (mut page, mut engine) = recorded(1);

    page.control(pressed(Button::Play));
    rendering(&mut engine, SECOND / 2);

    assert!(screen_of(&mut page).contains("0:00.5 / 0:01.0"));
}

#[test]
fn the_bar_fills_as_the_loop_plays() {
    let (mut page, mut engine) = recorded(1);

    page.control(pressed(Button::Play));
    rendering(&mut engine, SECOND / 2);

    let bar = bar_of(&mut page);
    assert_eq!(filled(&bar) * 2, bar.chars().count() - 2);
}

#[test]
fn a_stopped_loop_still_reads_what_was_recorded() {
    let (mut page, mut engine) = recorded(1);

    page.control(pressed(Button::Stop));
    rendering(&mut engine, SECOND);

    assert!(screen_of(&mut page).contains("0:01.0 / 0:01.0"));
}
