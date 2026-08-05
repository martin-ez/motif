//! Entry point for the `motif` binary.
//!
//! Composition only: it builds the pages, hands them to the shell, wraps that
//! in the monitor holding the audio device open, takes the terminal over, runs
//! the event loop, and reports why the run ended. The device opens before the
//! terminal does, so a host enumerating onto stderr does it to an ordinary
//! screen rather than over the drawn frame. The shell owns the pages and
//! quitting, and is the only way out of the mode the terminal is left in.
//!
//! What is left here is chrome the shell has no notion of. It takes the top row
//! and the bottom one off the region it was handed and gives the shell the rest,
//! so the pages beneath it cannot be drawn over and the chrome cannot land on a
//! row a page is using.

use std::process::ExitCode;

use motif::audio::{CpalBackend, StreamRequest, sample_clock};
use motif::device::DeviceProfile;
use motif::looper::LooperPage;
use motif::monitor::Monitor;
use motif::ui::{
    App, ControlEvent, EventLoop, Flow, Legend, Region, RenderError, Shell, TerminalScreen,
    columns_of,
};

const NAME: &str = concat!("motif ", env!("CARGO_PKG_VERSION"));
const QUIT: &str = "shift + stop to quit";
const CHROME_ROWS: usize = 1;

struct Chrome {
    shell: Shell,
}

fn write_right(region: &mut Region<'_>, text: &str) {
    let column = region.columns().saturating_sub(columns_of(text));

    region.write(column, 0, text);
}

impl App for Chrome {
    fn control(&mut self, event: ControlEvent) -> Flow {
        self.shell.control(event)
    }

    fn legend(&self) -> Legend {
        self.shell.legend()
    }

    fn draw(&mut self, region: Region<'_>) -> Flow {
        let (mut name, below) = region.split_top(CHROME_ROWS);
        let (pages, mut quit) = below.split_bottom(CHROME_ROWS);

        write_right(&mut name, NAME);
        write_right(&mut quit, QUIT);

        self.shell.draw(pages)
    }
}

fn requested() -> StreamRequest {
    let audio = DeviceProfile::TARGET.audio;

    StreamRequest {
        sample_rate: audio.sample_rate,
        block_size: audio.block_size,
    }
}

fn play() -> Result<(), RenderError> {
    let audio = DeviceProfile::TARGET.audio;
    let (looper, engine) = LooperPage::driving(audio, sample_clock(audio.sample_rate).1);
    let chrome = Chrome {
        shell: Shell::new([Box::new(looper)]),
    };
    let mut playing = Some(engine);
    let mut monitor = Monitor::opened(chrome, CpalBackend::new(), requested(), move || {
        playing.take()
    });

    let mut terminal = TerminalScreen::open()?;
    let (controls, mut screen) = terminal.split();

    EventLoop::new().run(&mut monitor, controls, &mut screen)?;

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
