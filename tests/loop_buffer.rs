//! The fixed store a captured loop lands in.
//!
//! Its capacity comes from the device profile and is decided before the stream
//! starts, so the facts worth stating are that the buffer is exactly as long as
//! the profile says, that frames come back as they went in, that a recording
//! longer than the buffer is reported short rather than growing the buffer or
//! panicking, and that recording allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

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
    assert_eq!(buffer.frames(), &[]);
}

#[test]
fn recorded_frames_are_read_back_in_the_order_they_arrived() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.1, 0.2, 0.3]);

    assert_eq!(buffer.frames(), &[0.1, 0.2, 0.3]);
}

#[test]
fn recording_reports_the_frames_it_took() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    assert_eq!(buffer.record(&[0.1, 0.2, 0.3]), 3);
}

#[test]
fn recording_appends_to_what_is_already_there() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.1, 0.2]);
    buffer.record(&[0.3]);

    assert_eq!(buffer.len(), 3);
    assert_eq!(buffer.frames(), &[0.1, 0.2, 0.3]);
}

#[test]
fn a_buffer_has_room_for_the_frames_it_has_not_recorded_yet() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.1, 0.2, 0.3]);

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
    assert_eq!(buffer.frames(), &overlong[..capacity]);
}

#[test]
fn a_full_buffer_records_nothing_further() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    let full = vec![1.0; buffer.capacity()];

    buffer.record(&full);
    let recorded = buffer.record(&[0.5, 0.5]);

    assert_eq!(recorded, 0);
    assert_eq!(buffer.vacant(), 0);
    assert_eq!(buffer.frames(), &full[..]);
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
