//! Entry point for the `motif` binary.
//!
//! Composition only: it builds the one link to the audio device and the pages,
//! hands the pages and the scheme that moves between them to the shell, wraps
//! that in the chrome and in the monitor that holds the link open, takes the
//! terminal over, runs the event loop, and reports why the run ended. The link
//! is built here because a run has one, whatever comes to share it, and it opens
//! before the terminal does so that a host enumerating onto stderr does it to an
//! ordinary screen. The shell owns the pages and keeps no gesture back, so what
//! ends a run is the terminal's own interrupt.
//!
//! The settings page is built before the monitor, because listing the devices
//! has to happen before a stream holds one of them.

use std::process::ExitCode;

use motif::audio::{
    AudioBackend, Counting, CpalBackend, DeviceLink, DeviceSelection, Escrow, SharedLink,
    StreamRequest, sample_clock,
};
use motif::device::DeviceProfile;
use motif::looper::LooperPage;
use motif::monitor::Monitor;
use motif::settings::AudioPage;
use motif::ui::{Chrome, EventLoop, RenderError, Scheme, Shell, TerminalScreen};

fn requested() -> StreamRequest {
    let audio = DeviceProfile::TARGET.audio;

    StreamRequest {
        sample_rate: audio.sample_rate,
        block_size: audio.block_size,
    }
}

fn play() -> Result<(), RenderError> {
    let audio = DeviceProfile::TARGET.audio;
    let (frames, elapsed) = sample_clock(audio.sample_rate);
    let (looper, engine, _takes) = LooperPage::driving(audio, elapsed);
    let playing = Escrow::holding(Counting::new(frames, engine));
    let backend = CpalBackend::new();
    let selection = backend
        .defaults(audio.sample_rate)
        .unwrap_or_else(DeviceSelection::nothing);
    let link = SharedLink::new(DeviceLink::new(
        backend,
        requested(),
        selection,
        move || playing.lend(),
    ));
    let settings = AudioPage::listing(link.clone());
    let shell = Shell::navigated_by([Box::new(looper), Box::new(settings)], Scheme::scenes());
    let mut monitor = Monitor::watching(Chrome::around(shell), Some(link));

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
