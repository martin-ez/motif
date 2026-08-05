//! Entry point for the `motif` binary.
//!
//! Composition only: it builds the pages, hands them to the shell, takes the
//! terminal over, runs the event loop against it, and reports why the run
//! ended. The shell owns the pages and quitting — the terminal is left in a
//! mode where the shell is the only way out of it.
//!
//! What is left here is chrome the shell has no notion of, drawn over the frame
//! after a page has had it. It is right-aligned, so that it lands beside what a
//! page draws from the left rather than on top of it. That is a convention
//! holding one shell and one page together, and it is what #216 replaces with a
//! page system that hands a page a region of its own.

use std::process::ExitCode;

use motif::audio::sample_clock;
use motif::device::DeviceProfile;
use motif::looper::{LooperPage, position_meter};
use motif::ui::{
    App, ControlEvent, EventLoop, Flow, Frame, Legend, RenderError, Shell, TerminalScreen,
    columns_of,
};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));
const QUIT: &str = "shift + stop to quit";

struct Chrome {
    shell: Shell,
}

fn last_row() -> usize {
    DeviceProfile::TARGET.screen.rows.saturating_sub(1)
}

fn write_right(frame: &mut Frame, row: usize, text: &str) {
    let screen = DeviceProfile::TARGET.screen;
    let column = screen.columns.saturating_sub(columns_of(text));

    frame.write(column, row, text);
}

impl App for Chrome {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.shell.control(event)
    }

    fn legend(&self) -> Legend {
        self.shell.legend()
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        let flow = self.shell.draw(frame);

        write_right(frame, 0, NAME);
        write_right(frame, last_row(), QUIT);

        flow
    }
}

fn play() -> Result<(), RenderError> {
    let mut terminal = TerminalScreen::open()?;
    let (controls, mut screen) = terminal.split();
    let looper = LooperPage::new(
        position_meter().1,
        sample_clock(DeviceProfile::TARGET.audio.sample_rate).1,
    );
    let mut chrome = Chrome {
        shell: Shell::new([Box::new(looper)]),
    };

    EventLoop::new().run(&mut chrome, controls, &mut screen)?;

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
