//! Open a duplex stream on the default devices, print what they granted, and
//! pass the input through to the output.
//!
//! A device rarely gives back exactly what it was asked for, and the numbers it
//! chose are what the rest of the system has to work against.
//!
//! ```sh
//! cargo run --example duplex
//! ```
//!
//! Nothing is audible until you ask for it. A stream opens stopped, and this
//! leaves it that way until Enter is pressed, because a microphone routed to a
//! speaker on the same machine feeds back and a laptop's own two are close
//! enough together to do it. Put headphones on first. Enter again stops it.
//!
//! Reaching the end of the input rather than a keypress — a piped or redirected
//! stdin, which is how anything automated runs this — is taken as the answer
//! no, so an unattended run passes no audio at all.

use std::io::{self, Write};

use motif::audio::{AudioBackend, CpalBackend, DeviceError, DuplexStream, StreamRequest};
use motif::device::DeviceProfile;

fn main() -> Result<(), DeviceError> {
    let profile = DeviceProfile::TARGET.audio;
    let request = StreamRequest {
        sample_rate: profile.sample_rate,
        block_size: profile.block_size,
    };
    println!(
        "requested  {} Hz, {} frames",
        request.sample_rate, request.block_size
    );

    let mut stream = CpalBackend::new().open(request)?;

    let granted = stream.config();
    println!(
        "granted    {} Hz, {} frames, {} in, {} out",
        granted.sample_rate, granted.block_size, granted.input_channels, granted.output_channels
    );

    println!("{:?}", stream.state());
    if !enter_pressed("headphones on? Enter passes audio through, Ctrl-C quits") {
        return Ok(());
    }

    stream.start()?;
    println!("{:?}", stream.state());
    enter_pressed("Enter stops");
    stream.stop()?;
    println!("{:?}", stream.state());

    Ok(())
}

fn enter_pressed(prompt: &str) -> bool {
    print!("{prompt} ");
    let shown = io::stdout().flush().is_ok();

    let mut line = String::new();
    let read = io::stdin().read_line(&mut line).unwrap_or(0);

    shown && read > 0
}
