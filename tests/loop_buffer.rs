//! The fixed store a captured loop lands in.
//!
//! Its capacity comes from the device profile and is decided before the stream
//! starts, so the facts worth stating are that the buffer is exactly as long as
//! the profile says, that frames come back as they went in, that a recording
//! longer than the buffer is reported short rather than growing the buffer or
//! panicking, and that recording allocates nothing.
//!
//! Layers are the other half. A loop is heard as the sum of the layers under it,
//! so what the tests state about them is what is heard: an overdub lies over the
//! take without lengthening it, undo takes the newest one away and leaves the
//! rest playing, the stack stops at a stated depth, and clear empties the whole
//! loop. Each of those runs on the thread that may not allocate, so they are
//! counted as well as checked.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use motif::device::{AudioProfile, DeviceProfile};
use motif::looper::LoopBuffer;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// An allocator that forwards to the system allocator and counts the calls
/// made by the thread that makes them.
///
/// SAFETY: every method hands its arguments to [`System`] unchanged and
/// returns what it returns, so the contract it upholds is the one `System`
/// already upholds. Counting touches only a thread-local `Cell<usize>`, which
/// is const-initialised and has no destructor, so it never allocates and never
/// re-enters the allocator.
///
/// Zeroed allocation is counted alongside plain allocation, because a loop
/// buffer is a block of silence and `Vec` asks for that one pre-zeroed — a
/// count that watched only [`GlobalAlloc::alloc`] would miss the regression
/// this is here to catch.
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

fn eight_frame_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 8,
        block_size: 4,
        max_loop_seconds: 1,
    }
}

/// The whole loop as it would be played: every layer summed, from the start.
///
/// The sample values below are binary fractions, so a sum of them is exact and
/// the comparison against one can be too.
fn heard(buffer: &LoopBuffer) -> Vec<f32> {
    let mut block = vec![0.0; buffer.len()];
    buffer.mix_into(&mut block, 0);

    block
}

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    black_box(vec![0.0_f32; 4]);
    let after = allocations();

    assert!(after > before, "the counter is not wired to the allocator");
}

#[test]
#[should_panic(expected = "frames")]
fn a_profile_with_no_loop_length_is_refused_at_setup() {
    LoopBuffer::for_profile(AudioProfile {
        sample_rate: 48_000,
        block_size: 256,
        max_loop_seconds: 0,
    });
}

#[test]
fn a_buffer_is_as_long_as_the_profiles_longest_loop() {
    let profile = eight_frame_profile();

    let buffer = LoopBuffer::for_profile(profile);

    assert_eq!(buffer.capacity(), profile.max_loop_frames());
}

#[test]
fn the_target_profile_sizes_a_buffer_of_its_own_maximum_loop() {
    let profile = DeviceProfile::TARGET.audio;

    let buffer = LoopBuffer::for_profile(profile);

    assert_eq!(buffer.capacity(), profile.max_loop_frames());
}

#[test]
fn a_new_buffer_holds_nothing() {
    let buffer = LoopBuffer::for_profile(eight_frame_profile());

    assert_eq!(buffer.len(), 0);
    assert!(buffer.is_empty());
    assert_eq!(buffer.depth(), 0);
    assert_eq!(heard(&buffer), []);
}

#[test]
fn a_buffer_holding_a_frame_is_not_empty() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.25]);

    assert!(!buffer.is_empty());
}

#[test]
fn recording_opens_the_take() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.25]);

    assert_eq!(buffer.depth(), 1);
}

#[test]
fn recorded_frames_are_read_back_in_the_order_they_arrived() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.25, 0.5, 0.75]);

    assert_eq!(heard(&buffer), [0.25, 0.5, 0.75]);
}

#[test]
fn recording_reports_the_frames_it_took() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    assert_eq!(buffer.record(&[0.25, 0.5, 0.75]), 3);
}

#[test]
fn recording_appends_to_what_is_already_there() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.25, 0.5]);
    buffer.record(&[0.75]);

    assert_eq!(buffer.len(), 3);
    assert_eq!(heard(&buffer), [0.25, 0.5, 0.75]);
}

#[test]
fn a_buffer_has_room_for_the_frames_it_has_not_recorded_yet() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.25, 0.5, 0.75]);

    assert_eq!(buffer.vacant(), buffer.capacity() - 3);
}

#[test]
fn a_recording_longer_than_the_buffer_keeps_the_frames_that_fit() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    let capacity = buffer.capacity();
    let overlong: Vec<f32> = (0..capacity + 4).map(|frame| frame as f32).collect();

    let recorded = buffer.record(&overlong);

    assert_eq!(recorded, capacity);
    assert_eq!(buffer.capacity(), capacity);
    assert_eq!(heard(&buffer), overlong[..capacity]);
}

#[test]
fn a_full_buffer_records_nothing_further() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    let full = vec![1.0; buffer.capacity()];

    buffer.record(&full);
    let recorded = buffer.record(&[0.5, 0.5]);

    assert_eq!(recorded, 0);
    assert_eq!(buffer.vacant(), 0);
    assert_eq!(heard(&buffer), full);
}

#[test]
fn recording_does_not_allocate() {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.5; profile.block_size as usize];

    let before = allocations();
    for _ in 0..8 {
        buffer.record(&block);
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn recording_into_a_full_buffer_does_not_allocate() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    let overlong = [0.5; 32];

    let before = allocations();
    buffer.record(&overlong);
    buffer.record(&overlong);
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn an_overdub_opens_a_layer_over_the_take() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25]);

    assert!(buffer.overdub());
    assert_eq!(buffer.depth(), 2);
}

#[test]
fn an_overdub_is_heard_on_top_of_the_take() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    buffer.overdub();
    buffer.record(&[0.125, 0.125]);

    assert_eq!(heard(&buffer), [0.375, 0.625]);
}

#[test]
fn an_overdub_shorter_than_the_loop_leaves_the_rest_of_the_take() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);

    buffer.overdub();
    buffer.record(&[0.125]);

    assert_eq!(heard(&buffer), [0.375, 0.5, 0.75]);
}

#[test]
fn an_overdub_has_room_for_the_loop_it_lies_over() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    buffer.overdub();

    assert_eq!(buffer.vacant(), 2);
}

#[test]
fn an_overdub_cannot_lengthen_the_loop() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    buffer.overdub();
    let recorded = buffer.record(&[0.125, 0.125, 0.125, 0.125]);

    assert_eq!(recorded, 2);
    assert_eq!(buffer.len(), 2);
}

#[test]
fn layers_stop_at_the_stated_bound() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25]);

    for _ in 1..LoopBuffer::LAYERS {
        assert!(buffer.overdub());
    }

    assert!(!buffer.overdub());
    assert_eq!(buffer.depth(), LoopBuffer::LAYERS);
}

#[test]
fn undo_takes_away_the_most_recent_overdub() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);

    assert!(buffer.undo());
    assert_eq!(buffer.depth(), 1);
    assert_eq!(heard(&buffer), [0.25, 0.5]);
}

#[test]
fn undo_leaves_the_layers_underneath_playing() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25]);
    buffer.overdub();
    buffer.record(&[0.125]);
    buffer.overdub();
    buffer.record(&[0.5]);

    buffer.undo();

    assert_eq!(heard(&buffer), [0.375]);
}

#[test]
fn undo_keeps_the_take() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    assert!(!buffer.undo());
    assert_eq!(buffer.depth(), 1);
    assert_eq!(heard(&buffer), [0.25, 0.5]);
}

#[test]
fn undo_makes_room_for_another_layer() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25]);
    for _ in 1..LoopBuffer::LAYERS {
        buffer.overdub();
    }

    buffer.undo();

    assert!(buffer.overdub());
}

#[test]
fn a_layer_opened_after_an_undo_starts_empty() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[1.0, 1.0]);
    buffer.undo();

    buffer.overdub();
    buffer.record(&[0.125]);

    assert_eq!(heard(&buffer), [0.375, 0.5]);
}

#[test]
fn clear_returns_the_buffer_to_idle() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    let capacity = buffer.capacity();
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125]);

    buffer.clear();

    assert_eq!(buffer.depth(), 0);
    assert_eq!(buffer.len(), 0);
    assert!(buffer.is_empty());
    assert_eq!(buffer.capacity(), capacity);
    assert_eq!(buffer.vacant(), capacity);
}

#[test]
fn a_cleared_buffer_takes_a_new_recording() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125]);
    buffer.clear();

    buffer.record(&[0.75]);

    assert_eq!(buffer.depth(), 1);
    assert_eq!(heard(&buffer), [0.75]);
}

#[test]
fn mixing_reports_the_frames_it_wrote() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    let mut block = [0.0; 3];

    assert_eq!(buffer.mix_into(&mut block, 0), 3);
}

#[test]
fn mixing_stops_at_the_end_of_the_loop() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    let mut block = [9.0; 4];

    let mixed = buffer.mix_into(&mut block, 0);

    assert_eq!(mixed, 2);
    assert_eq!(block, [0.25, 0.5, 9.0, 9.0]);
}

#[test]
fn the_loop_can_be_read_from_a_position_inside_it() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);
    let mut block = [0.0; 2];

    let mixed = buffer.mix_into(&mut block, 1);

    assert_eq!(mixed, 2);
    assert_eq!(block, [0.625, 0.75]);
}

#[test]
fn mixing_from_past_the_end_of_the_loop_writes_nothing() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    let mut block = [9.0; 2];

    let mixed = buffer.mix_into(&mut block, 4);

    assert_eq!(mixed, 0);
    assert_eq!(block, [9.0, 9.0]);
}

#[test]
fn mixing_an_empty_loop_writes_nothing() {
    let buffer = LoopBuffer::for_profile(eight_frame_profile());
    let mut block = [9.0; 2];

    let mixed = buffer.mix_into(&mut block, 0);

    assert_eq!(mixed, 0);
    assert_eq!(block, [9.0, 9.0]);
}

#[test]
fn layering_and_mixing_do_not_allocate() {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.5; profile.block_size as usize];
    let mut mixed = vec![0.0; profile.block_size as usize];
    buffer.record(&block);

    let before = allocations();
    for _ in 1..LoopBuffer::LAYERS {
        buffer.overdub();
        buffer.record(&block);
        buffer.mix_into(&mut mixed, 0);
    }
    while buffer.undo() {
        buffer.mix_into(&mut mixed, 0);
    }
    buffer.clear();
    let after = allocations();

    assert_eq!(after, before);
}
