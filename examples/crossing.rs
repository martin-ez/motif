//! Time a finished take crossing off the audio thread, a block at a time.
//!
//! ```sh
//! cargo run --release --example crossing
//! ```
//!
//! The crossing runs inside the audio callback, so what binds it is the block
//! period rather than anything analysis is measured against. The budget it is
//! reported against is stated beside it in `examples/README.md`.
//!
//! It measures the worst case on purpose: the longest loop the target profile
//! allows, with every layer of the stack laid over it, so what a block costs is
//! the most it can ever cost. No device is opened and nothing is heard.
//!
//! `--release` matters. An unoptimised build of a copy loop measures the build.

use std::time::{Duration, Instant};

use motif::device::{AudioProfile, DeviceProfile};
use motif::fixtures::harness;
use motif::looper::{LoopBuffer, TakeWriter, take_handoff};

const BUDGET_SHARE: u32 = 4;

fn block_period(profile: AudioProfile) -> Duration {
    Duration::from_secs_f64(f64::from(profile.block_size) / f64::from(profile.sample_rate))
}

fn take_length(profile: AudioProfile) -> Duration {
    Duration::from_secs(u64::from(profile.max_loop_seconds))
}

/// A full stack of layers over the longest loop `profile` allows.
fn worst_case_loop(profile: AudioProfile) -> LoopBuffer {
    let mut buffer = LoopBuffer::for_profile(profile);
    let take: Vec<f32> = (0..profile.max_loop_frames())
        .map(|frame| (frame as f32 * 0.001).sin() * 0.1)
        .collect();
    buffer.record(&take);

    while buffer.overdub(0) {
        buffer.record(&take);
    }

    buffer
}

/// Cross `buffer`'s take a block at a time, and report what each block took.
fn time_each_block(writer: &mut TakeWriter, buffer: &LoopBuffer, block: usize) -> Vec<Duration> {
    let mut blocks = Vec::with_capacity(TakeWriter::CROSSING_BLOCKS);
    writer.begin(buffer);

    loop {
        let started = Instant::now();
        let crossing = writer.advance(buffer, block);
        blocks.push(started.elapsed());

        if !crossing {
            return blocks;
        }
    }
}

fn median(sorted: &[Duration]) -> Duration {
    sorted.get(sorted.len() / 2).copied().unwrap_or_default()
}

fn share_of(cost: Duration, whole: Duration) -> f64 {
    100.0 * cost.as_secs_f64() / whole.as_secs_f64()
}

fn main() {
    let profile = DeviceProfile::TARGET.audio;
    let (mut writer, _reader) = take_handoff(profile);
    let buffer = worst_case_loop(profile);
    let block = profile.block_size as usize;

    let mut blocks = time_each_block(&mut writer, &buffer, block);
    blocks.sort_unstable();
    let worst = blocks.last().copied().unwrap_or_default();

    let period = block_period(profile);
    let budget = period / BUDGET_SHARE;
    let span = period * TakeWriter::CROSSING_BLOCKS as u32;
    let deadline = harness::deadline(take_length(profile));

    println!(
        "{} frames of {} layers, crossed in {} blocks of {block} frames",
        buffer.len(),
        buffer.depth(),
        blocks.len(),
    );
    println!("block period  {period:.2?}   budget {budget:.2?} (a quarter of it)");
    println!(
        "median block  {:.2?}   {:.1}% of the period",
        median(&blocks),
        share_of(median(&blocks), period),
    );
    println!(
        "worst block   {worst:.2?}   {:.1}% of the period",
        share_of(worst, period),
    );
    println!(
        "the crossing spans {span:.2?}, {:.1}% of the {deadline:.0?} deadline analysis has",
        share_of(span, deadline),
    );
    println!(
        "worst block is {} the budget",
        if worst <= budget { "inside" } else { "OVER" },
    );
}
