//! Time the spectral front end's transform over a whole take.
//!
//! ```sh
//! cargo run --release --example spectrum
//! ```
//!
//! Nothing here runs on the audio callback, so what binds it is the deadline
//! analysis answers in rather than the block period. The budget it is reported
//! against is stated beside it in `examples/README.md`.
//!
//! It measures the expensive end: the longest loop the target profile allows,
//! at each window the front end might be built on, hopped a quarter of a window
//! — the densest overlap in conventional use. No device is opened and nothing
//! is heard.
//!
//! `--release` matters. An unoptimised build of a butterfly loop measures the
//! build.

use std::hint::black_box;
use std::time::{Duration, Instant};

use motif::analysis::Transform;
use motif::device::{AudioProfile, DeviceProfile};
use motif::fixtures::harness;

const WINDOWS: [usize; 4] = [1_024, 2_048, 4_096, 8_192];
const BUDGET_SHARE: u32 = 10;
const OVERLAP: usize = 4;
const FUNDAMENTAL: f64 = 110.0;

fn take_length(profile: AudioProfile) -> Duration {
    Duration::from_secs(u64::from(profile.max_loop_seconds))
}

/// A held note over the longest loop `profile` allows, so that the strongest
/// bin says whether the transform found what was played.
fn played(profile: AudioProfile) -> Vec<f32> {
    (0..profile.max_loop_frames())
        .map(|frame| {
            let seconds = frame as f64 / f64::from(profile.sample_rate);
            let partial = |ratio: f64, level: f64| {
                level * (std::f64::consts::TAU * FUNDAMENTAL * ratio * seconds).sin()
            };

            (partial(1.0, 0.4) + partial(2.0, 0.2) + partial(3.0, 0.1)) as f32
        })
        .collect()
}

/// Transform every frame of `take`, and report what the lot took.
fn time_every_frame(transform: &Transform, take: &[f32], hop: usize) -> (Duration, usize) {
    let started = Instant::now();
    let mut frames = 0;

    for frame in take.windows(transform.window()).step_by(hop) {
        let magnitudes = transform
            .magnitudes(frame)
            .expect("a frame of the planned window");
        black_box(&magnitudes);
        frames += 1;
    }

    (started.elapsed(), frames)
}

fn loudest(magnitudes: &[f32]) -> usize {
    magnitudes
        .iter()
        .enumerate()
        .max_by(|(_, one), (_, other)| one.total_cmp(other))
        .map_or(0, |(bin, _)| bin)
}

fn frequency_of(bin: usize, window: usize, sample_rate: u32) -> f64 {
    bin as f64 * f64::from(sample_rate) / window as f64
}

fn share_of(cost: Duration, whole: Duration) -> f64 {
    100.0 * cost.as_secs_f64() / whole.as_secs_f64()
}

fn main() {
    let profile = DeviceProfile::TARGET.audio;
    let take = played(profile);
    let deadline = harness::deadline(take_length(profile));
    let budget = deadline / BUDGET_SHARE;

    println!(
        "a {:?} take at {} Hz: {deadline:.1?} of deadline, {budget:.2?} for the transform",
        take_length(profile),
        profile.sample_rate,
    );
    println!("a {FUNDAMENTAL} Hz note was played, so the strongest bin falls within one of it");
    println!();
    println!("window    hop  frames    each     total   of budget  strongest  within");

    for window in WINDOWS {
        let transform = Transform::of(window).expect("every candidate window is a power of two");
        let hop = window / OVERLAP;
        let (elapsed, frames) = time_every_frame(&transform, &take, hop);
        let each = elapsed.checked_div(frames as u32).unwrap_or_default();

        let opening = transform
            .magnitudes(&take[..window])
            .expect("a frame of the planned window");
        let strongest = frequency_of(loudest(&opening), window, profile.sample_rate);

        println!(
            "{window:>6} {hop:>6} {frames:>7} {each:>7.1?} {elapsed:>9.1?} {:>10.1}% {strongest:>8.1} Hz  {}",
            share_of(elapsed, budget),
            if elapsed <= budget { "inside" } else { "OVER" },
        );
    }
}
