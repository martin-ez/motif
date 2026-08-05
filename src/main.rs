//! Entry point for the `motif` binary.
//!
//! Composition only: it takes the terminal over, runs the event loop against
//! it, and reports why the run ended. The shell it runs owns the looper page,
//! keeps shift + stop to quit — the terminal is left in a mode where the shell
//! is the only way out of it — and hands every other control and the whole
//! frame to the page.
//!
//! The shell's own chrome is right-aligned, so that it lands beside what a page
//! draws from the left rather than on top of it. That is a convention holding
//! one shell and one page together, and it is what #216 replaces with a page
//! system that hands a page a region of its own.

use std::process::ExitCode;

use motif::device::{Button, DeviceProfile};
use motif::looper::{LooperPage, PositionReader, position_meter};
use motif::ui::{
    App, Cell, ControlEvent, EventLoop, Flow, Frame, Legend, RenderError, TerminalScreen,
};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));
const QUIT: &str = "shift + stop to quit";

struct Shell {
    looper: LooperPage,
}

impl Shell {
    fn new(position: PositionReader) -> Self {
        Self {
            looper: LooperPage::new(position),
        }
    }
}

fn last_row_above_the_legend() -> usize {
    DeviceProfile::TARGET
        .screen
        .rows
        .saturating_sub(Legend::ROWS + 1)
}

fn write_right(frame: &mut Frame, row: usize, text: &str) {
    let screen = DeviceProfile::TARGET.screen;
    let column = screen.columns.saturating_sub(text.chars().count());

    for (offset, glyph) in text.chars().enumerate() {
        frame.set(column + offset, row, Cell::new(glyph));
    }
}

impl App for Shell {
    fn control(&mut self, event: ControlEvent) -> Flow {
        match event {
            ControlEvent::Pressed {
                button: Button::Stop,
                shifted: true,
            } => Flow::Exit,
            _ => self.looper.control(event),
        }
    }

    fn legend(&self) -> Legend {
        self.looper.legend().answering(Button::Shift)
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        let flow = self.looper.draw(frame);

        write_right(frame, 0, NAME);
        write_right(frame, last_row_above_the_legend(), QUIT);

        flow
    }
}

fn play() -> Result<(), RenderError> {
    let mut terminal = TerminalScreen::open()?;
    let (controls, mut screen) = terminal.split();
    let mut shell = Shell::new(position_meter().1);

    EventLoop::new().run(&mut shell, controls, &mut screen)?;

    Ok(())
}

fn main() -> ExitCode {
    match play() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failed) => {
            eprintln!("motif: {failed}");
            ExitCode::FAILURE
        }
    }
}
