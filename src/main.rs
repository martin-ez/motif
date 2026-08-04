//! Entry point for the `motif` binary.
//!
//! Composition only: it takes the terminal over, runs the event loop against
//! it, and reports why the run ended. The shell it runs draws the program's
//! name and quits on a shifted stop, which is the whole of it until there are
//! pages to put in the frame.

use std::process::ExitCode;

use motif::device::Button;
use motif::ui::{App, Cell, ControlEvent, EventLoop, Flow, Frame, RenderError, TerminalScreen};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));

struct Shell;

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
        for (column, glyph) in NAME.chars().enumerate() {
            frame.set(column, 0, Cell::new(glyph));
        }

        Flow::Continue
    }
}

fn play() -> Result<(), RenderError> {
    let mut terminal = TerminalScreen::open()?;
    let (controls, screen) = terminal.split();

    EventLoop::new().run(&mut Shell, controls, screen)?;

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
