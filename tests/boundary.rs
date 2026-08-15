//! Carrying a block from the capture callback to the playback one, from both
//! ends of the ring that joins them.
//!
//! The two ends run on separate real-time threads in a real stream, but they
//! are pure with respect to each other: what one writes is what the other
//! reads, so the whole boundary is exercised here on one thread with no device.
//!
//! The facts worth stating are that a frame survives the fold to one sample and
//! the spread back out, that the path in between decides what is spread, that
//! only the selected channels carry it, that a full or dry ring is reported
//! rather than waited on, that nothing is due until both ends have run once,
//! that the slack outlasts a drifting clock and a dry ring, that the meter
//! reads what the path played, that nothing past full scale reaches the device,
//! and that neither callback allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::sync::{Arc, Mutex};

use motif::audio::{
    AudioPath, BlockCapture, BlockPlayback, ChannelSelection, Command, Levels, Passthrough,
    StreamConfig, boundary,
};
use motif::device::DeviceProfile;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// An allocator that forwards to the system allocator and counts the calls made
/// by the thread that makes them.
///
/// SAFETY: every method hands its arguments to [`System`] unchanged and returns
/// what it returns, so the contract it upholds is `System`'s. Counting touches
/// only a const-initialised thread-local `Cell<usize>` with no destructor, so it
/// never allocates and never re-enters the allocator.
///
/// Zeroed allocation is counted alongside plain allocation: the buffer a
/// callback reaches for is silence, which [`GlobalAlloc::alloc`] alone misses.
struct CountingAllocator;

#[expect(
    clippy::undocumented_unsafe_blocks,
    reason = "AGENTS.md 1.4 forbids the inline safety comment this lint asks for, so the argument is in the doc comment above instead"
)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

/// A path that plays one value, whatever it was handed.
struct Tone(f32);

impl AudioPath for Tone {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], playing: &mut [f32]) {
        playing.fill(self.0);
    }

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// A path that plays nothing, whatever it was handed.
///
/// A muted engine as the boundary sees one: frames arrive and none go out,
/// which is the case a meter on the input alone cannot tell from a live one.
struct Muted;

impl AudioPath for Muted {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// A path that keeps every frame it was handed and plays it on, so that a test
/// can read them back after it has been moved into the playback end.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<f32>>>);

impl Heard {
    fn frames(&self) -> Vec<f32> {
        self.0
            .lock()
            .expect("no test holds this across a panic")
            .clone()
    }
}

impl AudioPath for Heard {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.0
            .lock()
            .expect("no test holds this across a panic")
            .extend_from_slice(captured);
        playing.copy_from_slice(captured);
    }

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

fn config(input_channels: u16, output_channels: u16) -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 4,
        input_channels,
        output_channels,
    }
}

/// The frames a boundary on [`config`] with no slack carries between its ends,
/// which is two of its four-frame blocks.
const CARRIED: usize = 8;

fn running<P: AudioPath>(
    ends: (BlockCapture, BlockPlayback<P>),
) -> (BlockCapture, BlockPlayback<P>) {
    let (mut input, mut output) = ends;
    input.capture(&[]);
    output.render(&mut []);
    (input, output)
}

fn unstarted(config: StreamConfig, slack: usize) -> (BlockCapture, BlockPlayback<Passthrough>) {
    boundary(
        config,
        ChannelSelection::all(config.input_channels),
        ChannelSelection::all(config.output_channels),
        slack,
        Passthrough::new(),
    )
}

fn whole(config: StreamConfig, slack: usize) -> (BlockCapture, BlockPlayback<Passthrough>) {
    running(unstarted(config, slack))
}

fn played_through(
    config: StreamConfig,
    input: ChannelSelection,
    output: ChannelSelection,
) -> (BlockCapture, BlockPlayback<Passthrough>) {
    running(boundary(config, input, output, 0, Passthrough::new()))
}

fn channels(first: u16, count: u16) -> ChannelSelection {
    ChannelSelection { first, count }
}

/// One channel each way, at a block long enough for a correction to be a
/// fraction of it rather than the whole thing.
const BLOCK: usize = 64;

fn mono(block: usize) -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: block as u32,
        input_channels: 1,
        output_channels: 1,
    }
}

fn carrying(block: usize) -> (BlockCapture, BlockPlayback<Passthrough>) {
    whole(mono(block), block)
}

/// Run `blocks` blocks where the capture end delivers `captured` frames for
/// every `played` the playback end asks for, and report how many blocks came up
/// short in each direction.
///
/// Two devices on independent clocks differ by a fraction of a frame per block
/// rather than a whole one, so a whole frame is a drift far steeper than any
/// hardware's — which is what makes a run of a few hundred blocks stand in for
/// a listen of several minutes.
fn drifting(captured: usize, played: usize, blocks: usize) -> (usize, usize) {
    let (mut input, mut output) = carrying(BLOCK);
    let source = vec![0.5; captured];
    let mut sink = vec![0.0; played];
    let (mut dropped, mut starved) = (0, 0);

    for _ in 0..blocks {
        dropped += usize::from(input.capture(&source) < captured);
        starved += usize::from(output.render(&mut sink) < played);
    }

    (dropped, starved)
}

fn in_step(ends: &mut (BlockCapture, BlockPlayback<Passthrough>), blocks: usize) {
    let (input, output) = ends;
    let source = vec![0.5; BLOCK];
    let mut sink = vec![0.0; BLOCK];

    for _ in 0..blocks {
        input.capture(&source);
        output.render(&mut sink);
    }
}

fn drain(ends: &mut (BlockCapture, BlockPlayback<Passthrough>), renders: usize) {
    let mut sink = vec![0.0; BLOCK];

    for _ in 0..renders {
        ends.1.render(&mut sink);
    }
}

/// Play a block of `tone` through a boundary and report what the device was
/// handed, which is what the bound is stated on.
fn handed_to_the_device(tone: f32) -> [f32; 2] {
    let (mut input, mut output) = running(boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        Tone(tone),
    ));
    let mut played = [0.0; 2];

    input.capture(&[1.0, 1.0]);
    output.render(&mut played);

    played
}

/// Play half a block before the capture that feeds it, and say whether the
/// playback end came up short — which is the hiccup the slack exists to absorb.
fn played_early(ends: &mut (BlockCapture, BlockPlayback<Passthrough>)) -> bool {
    let (input, output) = ends;
    let source = vec![0.5; BLOCK / 2];
    let mut sink = vec![0.0; BLOCK / 2];

    let starved = output.render(&mut sink) < BLOCK / 2;
    input.capture(&source);
    starved
}

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    black_box(Vec::<f32>::with_capacity(4));
    let after = allocations();

    assert!(after > before, "the counter is not wired to the allocator");
}

#[test]
fn the_allocation_counter_counts_a_zeroed_allocation() {
    let before = allocations();
    black_box(vec![0.0_f32; 4]);
    let after = allocations();

    assert!(
        after > before,
        "the counter is not wired to zeroed allocation"
    );
}

#[test]
fn captured_frames_are_played_back_in_order() {
    let (mut input, mut output) = whole(config(1, 1), 0);
    let mut played = [0.0; 4];

    input.capture(&[0.1, 0.2, 0.3, 0.4]);
    output.render(&mut played);

    assert_eq!(played, [0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn a_frame_is_the_mean_of_the_channels_it_was_captured_on() {
    let (mut input, mut output) = whole(config(2, 1), 0);
    let mut played = [0.0; 2];

    input.capture(&[1.0, 0.0, 0.5, 0.5]);
    output.render(&mut played);

    assert_eq!(played, [0.5, 0.5]);
}

#[test]
fn a_frame_is_played_on_every_output_channel() {
    let (mut input, mut output) = whole(config(1, 2), 0);
    let mut played = [0.0; 4];

    input.capture(&[0.25, 0.75]);
    output.render(&mut played);

    assert_eq!(played, [0.25, 0.25, 0.75, 0.75]);
}

#[test]
fn a_frame_is_the_mean_of_the_selected_channels_alone() {
    let (mut input, mut output) =
        played_through(config(4, 1), channels(1, 2), ChannelSelection::all(1));
    let mut played = [0.0; 2];

    input.capture(&[9.0, 1.0, 0.0, 9.0, 9.0, 0.5, 0.5, 9.0]);
    output.render(&mut played);

    assert_eq!(played, [0.5, 0.5]);
}

#[test]
fn one_selected_channel_arrives_at_the_level_it_was_captured() {
    let (mut input, mut output) =
        played_through(config(2, 1), channels(0, 1), ChannelSelection::all(1));
    let mut played = [0.0; 2];

    input.capture(&[1.0, 0.0, 0.25, 0.0]);
    output.render(&mut played);

    assert_eq!(played, [1.0, 0.25]);
}

#[test]
fn an_output_channel_outside_the_selection_is_silent() {
    let (mut input, mut output) =
        played_through(config(1, 4), ChannelSelection::all(1), channels(1, 2));
    let mut played = [9.0; 8];

    input.capture(&[0.5, 0.25]);
    output.render(&mut played);

    assert_eq!(played, [0.0, 0.5, 0.5, 0.0, 0.0, 0.25, 0.25, 0.0]);
}

#[test]
fn a_path_is_handed_one_sample_for_every_captured_frame() {
    let heard = Heard::default();
    let (mut input, mut output) = running(boundary(
        config(2, 1),
        ChannelSelection::all(2),
        ChannelSelection::all(1),
        0,
        heard.clone(),
    ));

    input.capture(&[1.0, 0.0, 0.5, 0.5]);
    output.render(&mut [0.0; 2]);

    assert_eq!(heard.frames(), vec![0.5, 0.5]);
}

#[test]
fn what_a_path_plays_is_spread_rather_than_what_was_captured() {
    let (mut input, mut output) = running(boundary(
        config(1, 2),
        ChannelSelection::all(1),
        ChannelSelection::all(2),
        0,
        Tone(0.25),
    ));
    let mut played = [0.0; 4];

    input.capture(&[1.0, 1.0]);
    output.render(&mut played);

    assert_eq!(played, [0.25; 4]);
}

#[test]
fn a_path_is_handed_silence_for_the_frames_the_ring_could_not_supply() {
    let heard = Heard::default();
    let (mut input, mut output) = running(boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        heard.clone(),
    ));

    input.capture(&[1.0]);
    output.render(&mut [0.0; 3]);

    assert_eq!(heard.frames(), vec![1.0, 0.0, 0.0]);
}

#[test]
#[should_panic = "a stream carries nothing without channels"]
fn a_selection_of_no_channels_is_a_setup_error() {
    played_through(config(2, 2), channels(0, 0), ChannelSelection::all(2));
}

#[test]
#[should_panic = "a stream cannot reach a channel the device has not got"]
fn a_selection_past_the_end_of_the_device_is_a_setup_error() {
    played_through(config(2, 2), channels(1, 2), ChannelSelection::all(2));
}

#[test]
fn capture_reports_the_frames_it_took() {
    let (mut input, _output) = whole(config(2, 2), 0);

    assert_eq!(input.capture(&[0.0; 6]), 3);
}

#[test]
fn what_the_ring_cannot_supply_is_played_as_silence() {
    let (mut input, mut output) = whole(config(1, 1), 0);
    let mut played = [9.0; 4];

    input.capture(&[1.0]);
    let rendered = output.render(&mut played);

    assert_eq!(rendered, 1);
    assert_eq!(played, [1.0, 0.0, 0.0, 0.0]);
}

#[test]
fn a_ring_that_has_run_dry_does_not_repeat_itself() {
    let (mut input, mut output) = whole(config(1, 1), 0);
    let mut played = [0.0; 4];

    input.capture(&[1.0, 1.0, 1.0, 1.0]);
    output.render(&mut played);
    let rendered = output.render(&mut played);

    assert_eq!(rendered, 0);
    assert_eq!(played, [0.0; 4]);
}

#[test]
fn a_trailing_part_of_a_frame_is_silenced_rather_than_left() {
    let (mut input, mut output) = whole(config(1, 2), 0);
    let mut played = [9.0; 5];

    input.capture(&[0.5, 0.5]);
    output.render(&mut played);

    assert_eq!(played, [0.5, 0.5, 0.5, 0.5, 0.0]);
}

#[test]
fn an_output_too_short_for_one_frame_is_silenced() {
    let (mut input, mut output) = whole(config(1, 4), 0);
    let mut played = [9.0; 3];

    input.capture(&[0.5]);
    let rendered = output.render(&mut played);

    assert_eq!(rendered, 0);
    assert_eq!(played, [0.0; 3]);
}

#[test]
fn slack_delays_playback_by_that_many_frames() {
    let (mut input, mut output) = whole(config(1, 1), 2);
    let mut played = [0.0; 4];

    input.capture(&[0.1, 0.2]);
    output.render(&mut played);

    assert_eq!(played, [0.0, 0.0, 0.1, 0.2]);
}

#[test]
fn a_block_larger_than_the_configured_one_is_carried_whole() {
    let (mut input, mut output) = whole(config(1, 1), 0);
    let captured: Vec<f32> = (1..=8).map(|frame| frame as f32 / 10.0).collect();
    let mut played = [0.0; 8];

    assert_eq!(input.capture(&captured), 8);
    output.render(&mut played);

    assert_eq!(played.as_slice(), captured.as_slice());
}

#[test]
fn a_full_ring_drops_the_frames_it_cannot_hold() {
    let (mut input, _output) = whole(config(1, 1), 0);

    assert_eq!(input.capture(&[1.0; CARRIED + 1]), CARRIED);
}

#[test]
fn a_capture_before_the_playback_end_has_run_is_not_a_dropout() {
    let (mut input, _output) = unstarted(config(2, 1), 0);
    let frames = CARRIED + 1;

    assert_eq!(input.capture(&vec![1.0; frames * 2]), frames);
}

#[test]
fn a_playback_before_the_capture_end_has_run_is_not_a_dropout() {
    let (_input, mut output) = unstarted(config(1, 2), 0);
    let mut played = [9.0; 4];

    let supplied = output.render(&mut played);

    assert_eq!(supplied, 2);
    assert_eq!(played, [0.0; 4]);
}

#[test]
fn frames_captured_before_the_playback_end_ran_are_not_played() {
    let (mut input, mut output) = unstarted(config(1, 1), 0);
    let mut played = [0.0; 2];

    input.capture(&[0.1, 0.2]);
    output.render(&mut played);
    input.capture(&[0.3, 0.4]);
    output.render(&mut played);

    assert_eq!(played, [0.3, 0.4]);
}

#[test]
fn the_slack_survives_a_playback_that_ran_before_any_capture() {
    let (mut input, mut output) = unstarted(config(1, 1), 2);
    let mut played = [0.0; 4];

    output.render(&mut played);
    output.render(&mut played);
    input.capture(&[0.1, 0.2]);
    output.render(&mut played);

    assert_eq!(played, [0.0, 0.0, 0.1, 0.2]);
}

#[test]
fn the_slack_survives_a_capture_that_ran_before_any_playback() {
    let (mut input, mut output) = unstarted(config(1, 1), 2);
    let mut played = [0.0; 4];

    input.capture(&[0.8, 0.9]);
    output.render(&mut played);
    input.capture(&[0.1, 0.2]);
    output.render(&mut played);

    assert_eq!(played, [0.0, 0.0, 0.1, 0.2]);
}

#[test]
fn a_dry_ring_is_reported_short_once_the_boundary_is_carrying() {
    let (mut input, mut output) = whole(config(1, 1), 2);
    let mut played = [0.0; 4];
    input.capture(&[0.1, 0.2]);

    output.render(&mut played);
    let starved = output.render(&mut played);

    assert_eq!(starved, 0);
}

#[test]
fn a_restarted_boundary_does_not_count_its_start_as_a_dropout() {
    let (mut input, _output) = whole(config(1, 1), 0);
    let priming = input.priming();

    priming.restart();

    assert_eq!(input.capture(&[1.0; CARRIED + 1]), CARRIED + 1);
}

#[test]
fn a_restart_plays_silence_rather_than_what_the_last_run_left() {
    let (mut input, mut output) = whole(config(1, 1), 2);
    let priming = input.priming();
    let mut played = [0.0; 4];
    input.capture(&[0.7, 0.8, 0.9, 1.0]);

    priming.restart();
    output.render(&mut played);

    assert_eq!(played, [0.0; 4]);
}

#[test]
fn a_restarted_boundary_falls_back_to_its_slack() {
    let (mut input, mut output) = whole(config(1, 1), 2);
    let priming = input.priming();
    let mut played = [0.0; 4];
    input.capture(&[0.7, 0.8, 0.9, 1.0]);

    priming.restart();
    output.render(&mut played);
    input.capture(&[0.1, 0.2]);
    output.render(&mut played);

    assert_eq!(played, [0.9, 1.0, 0.1, 0.2]);
}

#[test]
fn a_capture_clock_running_fast_costs_no_frames() {
    let (dropped, starved) = drifting(BLOCK + 1, BLOCK, 400);

    assert_eq!((dropped, starved), (0, 0));
}

#[test]
fn a_capture_clock_running_slow_costs_no_frames() {
    let (dropped, starved) = drifting(BLOCK - 1, BLOCK, 400);

    assert_eq!((dropped, starved), (0, 0));
}

#[test]
fn a_drifting_clock_is_held_in_frames_rather_than_samples() {
    let stereo = StreamConfig {
        output_channels: 2,
        ..mono(BLOCK)
    };
    let (mut input, mut output) = whole(stereo, BLOCK);
    let source = vec![0.5; BLOCK - 1];
    let mut sink = vec![0.0; BLOCK * 2];
    let mut starved = 0;

    for _ in 0..400 {
        input.capture(&source);
        starved += usize::from(output.render(&mut sink) < BLOCK);
    }

    assert_eq!(starved, 0);
}

#[test]
fn a_hiccup_is_absorbed_by_the_slack() {
    let mut ends = carrying(BLOCK);
    in_step(&mut ends, 4);

    assert!(!played_early(&mut ends));
}

#[test]
fn what_the_boundary_holds_is_readable_from_the_playback_end() {
    let mut ends = carrying(BLOCK);
    let holding = ends.1.slack();
    in_step(&mut ends, 4);

    assert_eq!(holding.read().held, BLOCK);
}

#[test]
fn a_capture_clock_running_fast_is_paid_for_in_dropped_frames() {
    let mut ends = carrying(BLOCK);
    let holding = ends.1.slack();
    let source = vec![0.5; BLOCK + 1];
    let mut sink = vec![0.0; BLOCK];

    for _ in 0..200 {
        ends.0.capture(&source);
        ends.1.render(&mut sink);
    }

    assert!(holding.read().dropped > 0, "the drift was never corrected");
}

#[test]
fn a_capture_clock_running_slow_is_paid_for_in_inserted_frames() {
    let mut ends = carrying(BLOCK);
    let holding = ends.1.slack();
    let source = vec![0.5; BLOCK - 1];
    let mut sink = vec![0.0; BLOCK];

    for _ in 0..200 {
        ends.0.capture(&source);
        ends.1.render(&mut sink);
    }

    assert!(holding.read().inserted > 0, "the drift was never corrected");
}

#[test]
fn the_slack_comes_back_after_the_ring_has_run_dry() {
    let mut ends = carrying(BLOCK);
    in_step(&mut ends, 4);
    drain(&mut ends, 4);
    in_step(&mut ends, 200);

    assert!(!played_early(&mut ends));
}

#[test]
fn neither_callback_allocates() {
    let profile = DeviceProfile::TARGET.audio;
    let block = profile.block_size as usize;
    let (mut input, mut output) = whole(
        StreamConfig {
            sample_rate: profile.sample_rate,
            block_size: profile.block_size,
            input_channels: 2,
            output_channels: 2,
        },
        block,
    );
    let captured = vec![0.5; block * 2];
    let mut played = vec![0.0; block * 2];

    let before = allocations();
    for _ in 0..8 {
        input.capture(&captured);
        output.render(&mut played);
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn neither_callback_allocates_while_the_boundary_is_priming() {
    let (mut input, mut output) = whole(config(1, 1), 4);
    let priming = input.priming();
    let captured = [0.25; 32];
    let mut played = [0.0; 32];
    input.capture(&captured);

    let before = allocations();
    priming.restart();
    output.render(&mut played);
    input.capture(&captured);
    output.render(&mut played);
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn holding_the_slack_against_a_drifting_clock_allocates_nothing() {
    let (mut input, mut output) = carrying(BLOCK);
    let long = vec![0.5; BLOCK * 2];
    let short = vec![0.5; BLOCK / 2];
    let mut played = vec![0.0; BLOCK];

    let before = allocations();
    for _ in 0..8 {
        input.capture(&long);
        output.render(&mut played);
    }
    for _ in 0..8 {
        input.capture(&short);
        output.render(&mut played);
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn neither_callback_allocates_when_the_ring_is_full_or_dry() {
    let (mut input, mut output) = whole(config(1, 1), 0);
    let captured = [0.25; 32];
    let mut played = [0.0; 32];

    let before = allocations();
    input.capture(&captured);
    input.capture(&captured);
    output.render(&mut played);
    output.render(&mut played);
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn a_boundary_that_has_played_nothing_meters_silence() {
    let (_input, output) = unstarted(config(1, 1), 0);

    assert_eq!(output.metering().read(), Levels::SILENT);
}

#[test]
fn the_played_meter_reads_what_the_path_played() {
    let (mut input, mut output) = running(boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        Tone(0.5),
    ));

    input.capture(&[1.0, 1.0]);
    output.render(&mut [0.0; 2]);

    assert_eq!(
        output.metering().read(),
        Levels {
            peak: 0.5,
            rms: 0.5
        }
    );
}

#[test]
fn a_path_that_plays_nothing_meters_silence_under_a_loud_input() {
    let (mut input, mut output) = running(boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        Muted,
    ));

    input.capture(&[1.0, 1.0]);
    output.render(&mut [0.0; 2]);

    assert_eq!(output.metering().read(), Levels::SILENT);
}

#[test]
fn the_played_meter_is_taken_before_the_frames_are_spread() {
    let (mut input, mut output) = running(boundary(
        config(1, 2),
        ChannelSelection::all(1),
        channels(0, 1),
        0,
        Tone(1.0),
    ));

    input.capture(&[1.0, 1.0]);
    output.render(&mut [0.0; 4]);

    assert_eq!(output.metering().read().rms, 1.0);
}

#[test]
fn a_sample_inside_full_scale_reaches_the_device_as_the_path_played_it() {
    assert_eq!(handed_to_the_device(0.5), [0.5, 0.5]);
}

#[test]
fn a_sample_past_full_scale_reaches_the_device_at_full_scale() {
    assert_eq!(handed_to_the_device(4.0), [1.0, 1.0]);
}

#[test]
fn a_sample_under_full_scale_the_other_way_reaches_the_device_bounded() {
    assert_eq!(handed_to_the_device(-4.0), [-1.0, -1.0]);
}

#[test]
fn a_sample_that_is_not_a_number_reaches_the_device_as_silence() {
    assert_eq!(handed_to_the_device(f32::NAN), [0.0, 0.0]);
}

#[test]
fn an_infinite_sample_reaches_the_device_as_silence() {
    assert_eq!(handed_to_the_device(f32::INFINITY), [0.0, 0.0]);
    assert_eq!(handed_to_the_device(f32::NEG_INFINITY), [0.0, 0.0]);
}

#[test]
fn a_channel_outside_the_selection_stays_silent_under_a_bounded_sample() {
    let (mut input, mut output) = running(boundary(
        config(1, 2),
        ChannelSelection::all(1),
        channels(0, 1),
        0,
        Tone(4.0),
    ));
    let mut played = [0.0; 4];

    input.capture(&[1.0, 1.0]);
    output.render(&mut played);

    assert_eq!(played, [1.0, 0.0, 1.0, 0.0]);
}

#[test]
fn the_played_meter_reads_the_overshoot_the_path_wrote() {
    let (mut input, mut output) = running(boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        Tone(4.0),
    ));

    input.capture(&[1.0, 1.0]);
    output.render(&mut [0.0; 2]);

    assert_eq!(output.metering().read().peak, 4.0);
}

#[test]
fn a_boundary_that_went_back_to_priming_meters_the_silence_it_plays() {
    let (mut input, mut output) = running(boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        Tone(1.0),
    ));
    let priming = input.priming();
    input.capture(&[1.0, 1.0]);
    output.render(&mut [0.0; 2]);

    priming.restart();
    output.render(&mut [0.0; 2]);

    assert_eq!(output.metering().read(), Levels::SILENT);
}
