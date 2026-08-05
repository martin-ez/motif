//! Carrying a block from the capture callback to the playback one, from both
//! ends of the ring that joins them.
//!
//! The two ends run on separate real-time threads in a real stream, but they
//! are pure with respect to each other: what one writes is what the other
//! reads, so the whole boundary is exercised here on one thread with no device.
//!
//! The facts worth stating are that a frame survives the fold to one sample and
//! the spread back across channels, that the path in between decides the
//! samples that are spread, that only the selected channels carry them, that a
//! ring which is full or dry is reported rather than waited on, and that
//! neither callback allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::{Arc, Mutex};

use motif::audio::{
    AudioPath, BlockCapture, BlockPlayback, ChannelSelection, Passthrough, StreamConfig, boundary,
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
}

fn config(input_channels: u16, output_channels: u16) -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 4,
        input_channels,
        output_channels,
    }
}

fn whole(config: StreamConfig, slack: usize) -> (BlockCapture, BlockPlayback<Passthrough>) {
    boundary(
        config,
        ChannelSelection::all(config.input_channels),
        ChannelSelection::all(config.output_channels),
        slack,
        Passthrough::new(),
    )
}

fn played_through(
    config: StreamConfig,
    input: ChannelSelection,
    output: ChannelSelection,
) -> (BlockCapture, BlockPlayback<Passthrough>) {
    boundary(config, input, output, 0, Passthrough::new())
}

fn channels(first: u16, count: u16) -> ChannelSelection {
    ChannelSelection { first, count }
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
    let (mut input, mut output) = boundary(
        config(2, 1),
        ChannelSelection::all(2),
        ChannelSelection::all(1),
        0,
        heard.clone(),
    );

    input.capture(&[1.0, 0.0, 0.5, 0.5]);
    output.render(&mut [0.0; 2]);

    assert_eq!(heard.frames(), vec![0.5, 0.5]);
}

#[test]
fn what_a_path_plays_is_spread_rather_than_what_was_captured() {
    let (mut input, mut output) = boundary(
        config(1, 2),
        ChannelSelection::all(1),
        ChannelSelection::all(2),
        0,
        Tone(0.25),
    );
    let mut played = [0.0; 4];

    input.capture(&[1.0, 1.0]);
    output.render(&mut played);

    assert_eq!(played, [0.25; 4]);
}

#[test]
fn a_path_is_handed_silence_for_the_frames_the_ring_could_not_supply() {
    let heard = Heard::default();
    let (mut input, mut output) = boundary(
        config(1, 1),
        ChannelSelection::all(1),
        ChannelSelection::all(1),
        0,
        heard.clone(),
    );

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

    input.capture(&[1.0, 2.0]);
    output.render(&mut played);

    assert_eq!(played, [0.0, 0.0, 1.0, 2.0]);
}

#[test]
fn a_block_larger_than_the_configured_one_is_carried_whole() {
    let (mut input, mut output) = whole(config(1, 1), 0);
    let captured: Vec<f32> = (1..=8).map(|frame| frame as f32).collect();
    let mut played = [0.0; 8];

    assert_eq!(input.capture(&captured), 8);
    output.render(&mut played);

    assert_eq!(played.as_slice(), captured.as_slice());
}

#[test]
fn a_full_ring_drops_the_frames_it_cannot_hold() {
    let (mut input, _output) = whole(config(1, 1), 0);
    let capacity = input.capacity();

    assert_eq!(input.capture(&vec![1.0; capacity + 1]), capacity);
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
