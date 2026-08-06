//! Time the round trip out of the output and back into the input, over a cable
//! joining the two.
//!
//! ```sh
//! cargo run --example loopback
//! ```
//!
//! What it reports is the whole loop a monitored input goes round: the
//! boundary's slack, both converters, and whatever the device buffers at each
//! end. The budget it is measured against is stated beside it.
//!
//! Wire output to input before running it — a jack lead for a line interface,
//! or the headphone socket to the microphone socket. Failing that a speaker and
//! a microphone will do, and the figure then carries the air between them. The
//! click is one frame at full scale, so turn the output down first.
//!
//! Nothing is emitted until you ask for it, as in `duplex`: reaching the end of
//! the input rather than a keypress, which is how anything automated runs this,
//! is taken as the answer no.

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::{
    AudioBackend, CpalBackend, DeviceError, DuplexStream, RoundTrip, RoundTripReader,
    StreamRequest, latency_probe,
};
use motif::device::DeviceProfile;

const TAKES: u32 = 9;
const POLL: Duration = Duration::from_millis(20);
const GIVING_UP: Duration = Duration::from_secs(30);
const GRACE: Duration = Duration::from_secs(5);

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

    let backend = CpalBackend::new();
    let Some(selection) = backend.defaults(request.sample_rate) else {
        println!("no device to capture from and play to at that rate");
        return Ok(());
    };
    println!("host       {}", selection.host);
    println!("input      {}", selection.input);
    println!("output     {}", selection.output);

    let (probe, measured) = latency_probe();
    let mut stream = backend.open(&selection, request, probe)?;

    let granted = stream.config();
    println!(
        "granted    {} Hz, {} frames",
        granted.sample_rate, granted.block_size
    );

    let budget = RoundTrip::budget(request.block_size);
    println!(
        "budget     {} frames, {}",
        budget.frames,
        milliseconds(budget, granted.sample_rate)
    );

    if !enter_pressed("output wired to input? Enter clicks at full scale, Ctrl-C quits") {
        return Ok(());
    }

    stream.start()?;
    let taken = collected(&measured);
    stream.stop()?;

    report(&taken, budget, granted.sample_rate);

    Ok(())
}

fn collected(measured: &RoundTripReader) -> Vec<RoundTrip> {
    let started = Instant::now();
    let mut taken = Vec::new();
    let mut seen = 0;

    while (taken.len() as u32) < TAKES && started.elapsed() < GIVING_UP {
        if let Some(measurement) = measured.read()
            && measurement.takes > seen
        {
            seen = measurement.takes;
            taken.push(measurement.round_trip);
        }
        if taken.is_empty() && started.elapsed() > GRACE {
            break;
        }
        thread::sleep(POLL);
    }

    taken
}

fn report(taken: &[RoundTrip], budget: RoundTrip, sample_rate: u32) {
    if taken.is_empty() {
        println!("nothing came back — check that the output is wired to the input");
        return;
    }

    for (nth, round_trip) in taken.iter().enumerate() {
        println!(
            "take {}     {} frames, {}",
            nth + 1,
            round_trip.frames,
            milliseconds(*round_trip, sample_rate)
        );
    }

    let middle = median(taken);
    println!(
        "median     {} frames, {}, {} budget",
        middle.frames,
        milliseconds(middle, sample_rate),
        if middle.within(budget) {
            "within"
        } else {
            "over"
        }
    );
}

fn median(taken: &[RoundTrip]) -> RoundTrip {
    let mut frames: Vec<u32> = taken.iter().map(|round_trip| round_trip.frames).collect();
    frames.sort_unstable();

    RoundTrip {
        frames: frames.get(frames.len() / 2).copied().unwrap_or(0),
    }
}

fn milliseconds(round_trip: RoundTrip, sample_rate: u32) -> String {
    format!(
        "{:.2} ms",
        round_trip.duration(sample_rate).as_secs_f64() * 1_000.0
    )
}

fn enter_pressed(prompt: &str) -> bool {
    print!("{prompt} ");
    let shown = io::stdout().flush().is_ok();

    let mut line = String::new();
    let read = io::stdin().read_line(&mut line).unwrap_or(0);

    shown && read > 0
}
