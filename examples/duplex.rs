//! Open a duplex stream on the default devices and print what they granted.
//!
//! A device rarely gives back exactly what it was asked for, and the numbers it
//! chose are what the rest of the system has to work against.
//!
//! ```sh
//! cargo run --example duplex
//! ```
//!
//! The callback writes silence, so there is nothing to hear.

use std::thread::sleep;
use std::time::Duration;

use motif::audio::{AudioBackend, CpalBackend, DeviceError, DuplexStream, StreamRequest};
use motif::device::DeviceProfile;

const TIME_SPENT_RUNNING: Duration = Duration::from_secs(2);

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
