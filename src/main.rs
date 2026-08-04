//! Entry point for the `motif` binary.
//!
//! Composition only: it takes the terminal over, runs the event loop against
//! it, and reports why the run ended. The shell it runs draws the program's
//! name and the gesture that quits it, and quits on that gesture — which is the
//! whole of it until there are pages to put in the frame. The gesture is on
//! screen because the terminal is left in a mode where the shell is the only
//! way out of it.

use std::process::ExitCode;

use motif::device::Button;
use motif::ui::{App, Cell, ControlEvent, EventLoop, Flow, Frame, RenderError, TerminalScreen};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));
const QUIT: &str = "shift + stop to quit";

struct Shell;

fn write(frame: &mut Frame, row: usize, text: &str) {
    for (column, glyph) in text.chars().enumerate() {
        frame.set(column, row, Cell::new(glyph));
    }
}

impl App for Shell {
    fn control(&mut self, event: ControlEvent) -> Flow {
        match event {
            ControlEvent::Pressed {
                button: Button::Stop,
                shifted: true,
            } => Flow::Exit,
            _ => Flow::Continue,
        }
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        write(frame, 0, NAME);
        write(frame, 2, QUIT);

        Flow::Continue
    }
}

fn play() -> Result<(), RenderError> {
    let mut terminal = TerminalScreen::open()?;
    let (controls, mut screen) = terminal.split();

    EventLoop::new().run(&mut Shell, controls, &mut screen)?;

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
