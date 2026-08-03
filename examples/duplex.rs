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
//! Wear headphones. This is a microphone routed to a speaker, and a laptop's
//! own two are close enough together to feed back.

use std::thread::sleep;
use std::time::Duration;

use motif::audio::{AudioBackend, CpalBackend, DeviceError, DuplexStream, StreamRequest};
use motif::device::DeviceProfile;

const TIME_SPENT_RUNNING: Duration = Duration::from_secs(10);

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
    stream.start()?;
    println!("{:?} for {:?}", stream.state(), TIME_SPENT_RUNNING);
    sleep(TIME_SPENT_RUNNING);
    stream.stop()?;
    println!("{:?}", stream.state());

    Ok(())
}
