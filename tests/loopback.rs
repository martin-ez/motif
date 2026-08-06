//! Timing a click out of the output and back in at the input.
//!
//! Exercised against a delay line rather than a cable, so the round trip under
//! test is a number this file chose and the probe has to arrive at it. What is
//! worth stating is that a probe holds off until the stream has settled, that
//! the frames between a click and its return are what it reports, that it goes
//! on taking measurements, and that a return too quiet to be the click — or no
//! return at all — is not a measurement.
//!
//! A physical loop is at least a block long, since the boundary starts playback
//! a block behind capture, so every delay here is at least that.
//!
//! The last two run the probe through the real [`boundary`] instead, with its
//! output wired back to its input, which is what states that the slack the
//! boundary is built with is what the round trip costs.

use std::collections::VecDeque;
use std::time::Duration;

use motif::audio::{
    AudioPath, ChannelSelection, Command, LatencyProbe, RoundTrip, RoundTripReader, StreamConfig,
    boundary, latency_probe,
};
use motif::device::DeviceProfile;

const RATE: u32 = 1_000;
const BLOCK: usize = 50;

/// Blocks of silence before the first click, pinned here rather than derived
/// from the probe's own settling time: a test that counts in the number under
/// test turns a mutant into a hang.
const SETTLE_BLOCKS: usize = 5;

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: RATE,
        block_size: BLOCK as u32,
        input_channels: 1,
        output_channels: 1,
    }
}

/// A probe with its output wired back to its input through `delay` frames.
struct Loopback {
    probe: LatencyProbe,
    reader: RoundTripReader,
    line: VecDeque<f32>,
    gain: f32,
    played: Vec<Vec<f32>>,
}

impl Loopback {
    fn open(delay: usize, gain: f32) -> Self {
        let (mut probe, reader) = latency_probe();
        probe.prepare(config());

        Self {
            probe,
            reader,
            line: VecDeque::from(vec![0.0; delay]),
            gain,
            played: Vec::new(),
        }
    }

    fn wired(delay: usize) -> Self {
        Self::open(delay, 1.0)
    }

    fn unplugged() -> Self {
        Self::open(BLOCK, 0.0)
    }

    fn block(&mut self) {
        let captured: Vec<f32> = (0..BLOCK)
            .map(|_| self.line.pop_front().unwrap_or(0.0) * self.gain)
            .collect();
        let mut playing = vec![0.0; BLOCK];

        self.probe.render(&captured, &mut playing);

        self.line.extend(playing.iter().copied());
        self.played.push(playing);
    }

    fn run(&mut self, blocks: usize) -> &mut Self {
        for _ in 0..blocks {
            self.block();
        }
        self
    }

    fn clicked_in(&self, block: usize) -> bool {
        self.played[block].iter().any(|sample| *sample != 0.0)
    }

    fn clicks(&self) -> Vec<usize> {
        (0..self.played.len())
            .filter(|block| self.clicked_in(*block))
            .collect()
    }
}

#[test]
fn a_probe_reports_nothing_before_its_first_take() {
    let loopback = Loopback::wired(BLOCK);

    assert!(loopback.reader.read().is_none());
}

#[test]
fn a_probe_plays_nothing_before_it_has_settled() {
    let mut loopback = Loopback::wired(BLOCK);

    loopback.run(SETTLE_BLOCKS);

    assert!((0..SETTLE_BLOCKS).all(|block| !loopback.clicked_in(block)));
}

#[test]
fn a_probe_clicks_once_it_has_settled() {
    let mut loopback = Loopback::wired(BLOCK);

    loopback.run(SETTLE_BLOCKS + 1);

    assert!(loopback.clicked_in(SETTLE_BLOCKS));
}

#[test]
fn a_click_is_one_frame_at_the_start_of_its_block() {
    let mut loopback = Loopback::wired(BLOCK);

    loopback.run(SETTLE_BLOCKS + 1);
    let click = &loopback.played[SETTLE_BLOCKS];

    assert_eq!(click[0].abs(), 1.0);
    assert!(click[1..].iter().all(|sample| *sample == 0.0));
}

#[test]
fn a_click_returning_after_a_delay_measures_that_delay() {
    let mut loopback = Loopback::wired(120);

    loopback.run(8);

    let measured = loopback.reader.read().expect("the click came back");
    assert_eq!(measured.round_trip.frames, 120);
}

#[test]
fn a_measured_round_trip_is_the_first_take() {
    let mut loopback = Loopback::wired(120);

    loopback.run(8);

    let measured = loopback.reader.read().expect("the click came back");
    assert_eq!(measured.takes, 1);
}

#[test]
fn a_probe_takes_another_measurement_after_the_first() {
    let mut loopback = Loopback::wired(120);

    loopback.run(16);

    let measured = loopback.reader.read().expect("both clicks came back");
    assert_eq!(measured.takes, 2);
}

#[test]
fn a_second_take_measures_the_same_loop() {
    let mut loopback = Loopback::wired(120);

    loopback.run(16);

    let measured = loopback.reader.read().expect("both clicks came back");
    assert_eq!(measured.round_trip.frames, 120);
}

#[test]
fn a_return_quieter_than_the_threshold_is_not_a_return() {
    let mut loopback = Loopback::open(120, 0.15);

    loopback.run(16);

    assert!(loopback.reader.read().is_none());
}

#[test]
fn a_return_at_the_threshold_is_a_return() {
    let mut loopback = Loopback::open(120, 0.2);

    loopback.run(8);

    let measured = loopback
        .reader
        .read()
        .expect("the click came back at the threshold");
    assert_eq!(measured.round_trip.frames, 120);
}

#[test]
fn a_click_that_never_returns_reports_no_measurement() {
    let mut loopback = Loopback::unplugged();

    loopback.run(16);

    assert!(loopback.reader.read().is_none());
}

#[test]
fn a_probe_that_hears_no_return_settles_before_clicking_again() {
    let mut loopback = Loopback::unplugged();

    loopback.run(16);

    assert_eq!(loopback.clicks(), vec![SETTLE_BLOCKS, 15]);
}

#[test]
fn a_probe_bounds_its_click_by_the_block_it_was_handed() {
    let mut loopback = Loopback::wired(BLOCK);

    loopback.run(SETTLE_BLOCKS);
    loopback.probe.render(&[], &mut []);
    loopback.run(1);

    assert!(loopback.clicked_in(SETTLE_BLOCKS));
}

#[test]
fn a_probe_answers_no_command() {
    let (mut probe, _reader) = latency_probe();

    let answered = [
        Command::SetGain(0.5),
        Command::SetMuted(true),
        Command::Undo,
        Command::Clear,
    ]
    .into_iter()
    .any(|command| probe.apply(command));

    assert!(!answered);
}

/// Blocks to run a wired boundary for: enough to prime it, settle the probe and
/// carry a click back, pinned here rather than derived from any of the three.
const BOUNDARY_BLOCKS: usize = 20;

/// The wiring cannot capture a block before it has been played, so every
/// measurement taken through it carries one block that is the harness's own and
/// not the boundary's.
const WIRING: usize = BLOCK;

/// Run a boundary built with `slack` until its own click comes back.
fn round_trip_through_boundary(slack: usize) -> RoundTrip {
    let (probe, measured) = latency_probe();
    let (mut input, mut output) = boundary(
        config(),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        slack,
        probe,
    );
    let mut wire = vec![0.0; BLOCK];

    for _ in 0..BOUNDARY_BLOCKS {
        input.capture(&wire);
        output.render(&mut wire);

        if let Some(measurement) = measured.read() {
            return measurement.round_trip;
        }
    }

    panic!("the click never came back through the boundary");
}

#[test]
fn a_round_trip_through_the_boundary_carries_its_slack() {
    let measured = round_trip_through_boundary(2 * BLOCK);

    assert_eq!(measured.frames, (WIRING + 2 * BLOCK) as u32);
}

#[test]
fn slack_in_the_boundary_lengthens_the_round_trip_by_its_own_length() {
    let tight = round_trip_through_boundary(0).frames;
    let slack = round_trip_through_boundary(BLOCK).frames;

    assert_eq!(slack - tight, BLOCK as u32);
}

#[test]
fn a_round_trip_is_a_duration_at_the_rate_it_was_measured_at() {
    let round_trip = RoundTrip { frames: 48 };

    assert_eq!(round_trip.duration(48_000), Duration::from_millis(1));
}

#[test]
fn a_round_trip_at_no_rate_at_all_is_no_time_at_all() {
    let round_trip = RoundTrip { frames: 48 };

    assert_eq!(round_trip.duration(0), Duration::ZERO);
}

#[test]
fn a_budget_is_five_blocks_of_the_size_the_stream_granted() {
    let block_size = DeviceProfile::TARGET.audio.block_size;

    assert_eq!(RoundTrip::budget(block_size).frames, 5 * block_size);
}

#[test]
fn the_target_profile_budgets_under_twenty_seven_milliseconds() {
    let audio = DeviceProfile::TARGET.audio;

    let budget = RoundTrip::budget(audio.block_size).duration(audio.sample_rate);

    assert_eq!(budget, Duration::from_nanos(26_666_666));
}

#[test]
fn a_round_trip_inside_the_budget_is_within_it() {
    let budget = RoundTrip::budget(256);

    assert!(RoundTrip { frames: 1_279 }.within(budget));
}

#[test]
fn a_round_trip_exactly_at_the_budget_is_within_it() {
    let budget = RoundTrip::budget(256);

    assert!(RoundTrip { frames: 1_280 }.within(budget));
}

#[test]
fn a_round_trip_over_the_budget_is_not_within_it() {
    let budget = RoundTrip::budget(256);

    assert!(!RoundTrip { frames: 1_281 }.within(budget));
}
