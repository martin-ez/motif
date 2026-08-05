//! Passing captured audio through to the output, from both ends of the ring
//! that carries it.
//!
//! The two ends run on separate real-time threads in a real stream, but they
//! are pure with respect to each other: what one writes is what the other
//! reads, so the whole path is exercised here on one thread with no device.
//!
//! The facts worth stating are that a frame survives the fold to one sample and
//! the spread back across channels, that a ring which is full or dry is
//! reported rather than waited on, and that neither callback allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use motif::audio::{StreamConfig, passthrough};
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

fn config(input_channels: u16, output_channels: u16) -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 4,
        input_channels,
        output_channels,
    }
}

#[test]
fn captured_frames_are_played_back_in_order() {
    let (mut input, mut output) = passthrough(config(1, 1), 0);
    let mut played = [0.0; 4];

    input.capture(&[0.1, 0.2, 0.3, 0.4]);
    output.render(&mut played);

    assert_eq!(played, [0.1, 0.2, 0.3, 0.4]);
}

#[test]
fn a_frame_is_the_mean_of_the_channels_it_was_captured_on() {
    let (mut input, mut output) = passthrough(config(2, 1), 0);
    let mut played = [0.0; 2];

    input.capture(&[1.0, 0.0, 0.5, 0.5]);
    output.render(&mut played);

    assert_eq!(played, [0.5, 0.5]);
}

#[test]
fn a_frame_is_played_on_every_output_channel() {
    let (mut input, mut output) = passthrough(config(1, 2), 0);
    let mut played = [0.0; 4];

    input.capture(&[0.25, 0.75]);
    output.render(&mut played);

    assert_eq!(played, [0.25, 0.25, 0.75, 0.75]);
}

#[test]
fn capture_reports_the_frames_it_took() {
    let (mut input, _output) = passthrough(config(2, 2), 0);

    assert_eq!(input.capture(&[0.0; 6]), 3);
}

#[test]
fn what_the_ring_cannot_supply_is_played_as_silence() {
    let (mut input, mut output) = passthrough(config(1, 1), 0);
    let mut played = [9.0; 4];

    input.capture(&[1.0]);
    let rendered = output.render(&mut played);

    assert_eq!(rendered, 1);
    assert_eq!(played, [1.0, 0.0, 0.0, 0.0]);
}

#[test]
fn a_ring_that_has_run_dry_does_not_repeat_itself() {
    let (mut input, mut output) = passthrough(config(1, 1), 0);
    let mut played = [0.0; 4];

    input.capture(&[1.0, 1.0, 1.0, 1.0]);
    output.render(&mut played);
    let rendered = output.render(&mut played);

    assert_eq!(rendered, 0);
    assert_eq!(played, [0.0; 4]);
}

#[test]
fn a_trailing_part_of_a_frame_is_silenced_rather_than_left() {
    let (mut input, mut output) = passthrough(config(1, 2), 0);
    let mut played = [9.0; 5];

    input.capture(&[0.5, 0.5]);
    output.render(&mut played);

    assert_eq!(played, [0.5, 0.5, 0.5, 0.5, 0.0]);
}

#[test]
fn an_output_too_short_for_one_frame_is_silenced() {
    let (mut input, mut output) = passthrough(config(1, 4), 0);
    let mut played = [9.0; 3];

    input.capture(&[0.5]);
    let rendered = output.render(&mut played);

    assert_eq!(rendered, 0);
    assert_eq!(played, [0.0; 3]);
}

#[test]
fn slack_delays_playback_by_that_many_frames() {
    let (mut input, mut output) = passthrough(config(1, 1), 2);
    let mut played = [0.0; 4];

    input.capture(&[1.0, 2.0]);
    output.render(&mut played);

    assert_eq!(played, [0.0, 0.0, 1.0, 2.0]);
}

#[test]
fn a_block_larger_than_the_configured_one_is_carried_whole() {
    let (mut input, mut output) = passthrough(config(1, 1), 0);
    let captured: Vec<f32> = (1..=8).map(|frame| frame as f32).collect();
    let mut played = [0.0; 8];

    assert_eq!(input.capture(&captured), 8);
    output.render(&mut played);

    assert_eq!(played.as_slice(), captured.as_slice());
}

#[test]
fn a_full_ring_drops_the_frames_it_cannot_hold() {
    let (mut input, _output) = passthrough(config(1, 1), 0);
    let capacity = input.capacity();

    assert_eq!(input.capture(&vec![1.0; capacity + 1]), capacity);
}

#[test]
fn neither_callback_allocates() {
    let profile = DeviceProfile::TARGET.audio;
    let block = profile.block_size as usize;
    let (mut input, mut output) = passthrough(
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
    let (mut input, mut output) = passthrough(config(1, 1), 0);
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
