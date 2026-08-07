//! The route a finished take takes off the audio thread.
//!
//! The loop cannot cross as it stands — it is megabytes owned by a callback
//! that may not allocate, lock or block — so the facts worth stating are that
//! what comes out the far end is the loop as it was heard, layers and all, that
//! it crosses in bounded steps rather than one copy, and that nothing partial
//! is ever visible.
//!
//! The other half is what happens when the player carries on. A take being read
//! belongs to the reader until it lets go, and the takes published behind it
//! neither wait for it nor disturb it.
//!
//! The publishing end runs in the callback, so the allocations are counted too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use motif::device::AudioProfile;
use motif::looper::{FinishedTake, LoopBuffer, TakeWriter, take_handoff};

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
/// Zeroed allocation is counted alongside plain allocation: the slots a take
/// crosses in are blocks of silence, which [`GlobalAlloc::alloc`] alone misses.
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

/// More advances than any handoff in these tests can need, so a crossing that
/// never ends fails rather than hanging.
///
/// Pinned here rather than read off `TakeWriter::CROSSING_BLOCKS`, which a
/// mutant is free to make enormous.
const ADVANCES_ALLOWED: usize = 512;

fn eight_frame_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 8,
        block_size: 4,
        max_loop_seconds: 1,
    }
}

/// A profile whose longest loop is more frames than a take gets blocks to cross
/// in, and an odd number of them.
///
/// A share is then several frames rather than one, and the last share of a full
/// loop is a part share.
fn odd_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 65,
        block_size: 4,
        max_loop_seconds: 1,
    }
}

fn four_frame_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 4,
        block_size: 4,
        max_loop_seconds: 1,
    }
}

/// A profile whose longest loop is twice the blocks a take gets to cross in, so
/// a full block's share is two frames and half a block's is one.
///
/// A share of one frame is the floor, so a profile any smaller than this cannot
/// show a share following the block it was handed.
fn scaling_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 128,
        block_size: 8,
        max_loop_seconds: 1,
    }
}

/// A signal of `frames` frames, no two of them alike, so a frame that crossed
/// out of place is a frame that fails.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames).map(|frame| frame as f32 / 256.0).collect()
}

/// A buffer over `profile`'s longest loop, full to its last frame.
fn full_loop(profile: AudioProfile) -> LoopBuffer {
    let mut buffer = LoopBuffer::for_profile(profile);
    buffer.record(&ramp(profile.max_loop_frames()));

    buffer
}

/// A buffer over an eight-frame loop, holding `captured`.
fn recorded(captured: &[f32]) -> LoopBuffer {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(captured);

    buffer
}

/// The block every profile here but [`scaling_profile`] states, pinned so that
/// a crossing is handed the frames its writer was built to expect.
const A_BLOCK: usize = 4;

/// Hand `buffer`'s loop across, `handed` frames of it a block, as a callback
/// given that many frames does, and report how many blocks that took.
fn cross_in_blocks_of(writer: &mut TakeWriter, buffer: &LoopBuffer, handed: usize) -> usize {
    writer.begin(buffer);

    for advance in 0..ADVANCES_ALLOWED {
        if !writer.advance(buffer, handed) {
            return advance + 1;
        }
    }

    panic!("the handoff never finished crossing");
}

/// Hand `buffer`'s loop across a block at a time, as the callback does, and
/// report how many blocks that took.
fn cross(writer: &mut TakeWriter, buffer: &LoopBuffer) -> usize {
    cross_in_blocks_of(writer, buffer, A_BLOCK)
}

fn samples(take: &FinishedTake<'_>) -> Vec<f32> {
    take.samples().collect()
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
#[should_panic(expected = "block")]
fn a_profile_with_no_block_is_refused_at_setup() {
    take_handoff(AudioProfile {
        block_size: 0,
        ..eight_frame_profile()
    });
}

#[test]
fn a_handoff_nothing_has_crossed_hands_over_nothing() {
    let (_writer, mut reader) = take_handoff(eight_frame_profile());

    assert!(reader.claim().is_none());
}

#[test]
fn a_finished_take_crosses_as_it_was_recorded() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75]);

    cross(&mut writer, &buffer);

    let take = reader.claim().expect("the take crossed");
    assert_eq!(samples(&take), [0.25, 0.5, 0.75]);
}

#[test]
fn a_take_carries_the_frames_it_crossed_with() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75]);

    cross(&mut writer, &buffer);

    let take = reader.claim().expect("the take crossed");
    assert_eq!(take.frames(), 3);
}

#[test]
fn the_layers_of_a_take_cross_summed_into_one_signal() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let mut buffer = recorded(&[0.25, 0.5]);
    buffer.overdub(1);
    buffer.record(&[0.125]);

    cross(&mut writer, &buffer);

    let take = reader.claim().expect("the take crossed");
    assert_eq!(samples(&take), [0.25, 0.625]);
}

#[test]
fn a_take_of_no_frames_crosses_nothing() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let buffer = LoopBuffer::for_profile(eight_frame_profile());

    cross(&mut writer, &buffer);

    assert!(reader.claim().is_none());
}

#[test]
fn a_take_crosses_a_chunk_at_a_time() {
    let (mut writer, _reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75, 1.0]);

    assert!(
        cross(&mut writer, &buffer) > 1,
        "the take crossed in one go"
    );
}

#[test]
fn a_take_crosses_within_the_blocks_it_is_given() {
    let (mut writer, _reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75, 1.0, 0.25, 0.5, 0.75, 1.0]);

    assert!(cross(&mut writer, &buffer) <= TakeWriter::CROSSING_BLOCKS);
}

#[test]
fn half_a_take_is_not_a_take() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75, 1.0]);

    writer.begin(&buffer);
    writer.advance(&buffer, A_BLOCK);

    assert!(reader.claim().is_none());
}

#[test]
fn an_abandoned_crossing_hands_over_nothing() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75, 1.0]);

    writer.begin(&buffer);
    writer.advance(&buffer, A_BLOCK);
    writer.abandon();
    for _ in 0..ADVANCES_ALLOWED {
        writer.advance(&buffer, A_BLOCK);
    }

    assert!(reader.claim().is_none());
}

#[test]
fn an_abandoned_crossing_leaves_the_take_before_it_where_it_was() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let crossed = recorded(&[0.25, 0.5]);
    let abandoned = recorded(&[0.75, 1.0]);

    cross(&mut writer, &crossed);
    writer.begin(&abandoned);
    writer.advance(&abandoned, A_BLOCK);
    writer.abandon();

    let take = reader.claim().expect("the first take crossed");
    assert_eq!(samples(&take), [0.25, 0.5]);
}

#[test]
fn a_take_is_handed_over_once() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5]);

    cross(&mut writer, &buffer);
    reader.claim().expect("the take crossed");

    assert!(reader.claim().is_none());
}

#[test]
fn a_take_being_read_survives_the_take_that_follows_it() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let first = recorded(&[0.25, 0.5]);
    let second = recorded(&[0.75, 1.0]);

    cross(&mut writer, &first);
    let take = reader.claim().expect("the first take crossed");
    cross(&mut writer, &second);

    assert_eq!(samples(&take), [0.25, 0.5]);
}

#[test]
fn the_take_after_the_one_being_read_crosses_whole() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let first = recorded(&[0.25, 0.5]);
    let second = recorded(&[0.375, 0.625]);

    cross(&mut writer, &first);
    let read = samples(&reader.claim().expect("the first take crossed"));
    cross(&mut writer, &second);

    assert_eq!(read, [0.25, 0.5]);
    let take = reader.claim().expect("the second take crossed");
    assert_eq!(samples(&take), [0.375, 0.625]);
}

#[test]
fn a_take_nobody_looked_at_is_gone() {
    let (mut writer, mut reader) = take_handoff(eight_frame_profile());
    let first = recorded(&[0.25, 0.5]);
    let unread = recorded(&[0.5, 0.75]);
    let newest = recorded(&[0.375, 0.625]);

    cross(&mut writer, &first);
    {
        let held = reader.claim().expect("the first take crossed");
        cross(&mut writer, &unread);
        cross(&mut writer, &newest);
        assert_eq!(samples(&held), [0.25, 0.5]);
    }

    let take = reader.claim().expect("the newest take crossed");
    assert_eq!(samples(&take), [0.375, 0.625]);
}

#[test]
fn a_take_that_fills_the_buffer_crosses_to_its_last_frame() {
    let profile = odd_profile();
    let (mut writer, mut reader) = take_handoff(profile);
    let buffer = full_loop(profile);

    cross(&mut writer, &buffer);

    let crossed = reader.claim().expect("the take crossed");
    assert_eq!(samples(&crossed), ramp(profile.max_loop_frames()));
}

#[test]
fn a_take_longer_than_the_handoff_crosses_as_much_as_fits() {
    let (mut writer, mut reader) = take_handoff(four_frame_profile());
    let buffer = recorded(&[0.125, 0.25, 0.375, 0.5, 0.0625, 0.125]);

    cross(&mut writer, &buffer);

    let take = reader.claim().expect("as much of the take as fits crossed");
    assert_eq!(samples(&take), [0.125, 0.25, 0.375, 0.5]);
}

#[test]
fn a_take_crossing_at_half_the_block_takes_twice_the_blocks() {
    let profile = scaling_profile();
    let (mut writer, _reader) = take_handoff(profile);
    let buffer = full_loop(profile);
    let whole = cross_in_blocks_of(&mut writer, &buffer, 8);

    let halved = cross_in_blocks_of(&mut writer, &buffer, 4);

    assert_eq!(halved, 2 * whole);
}

#[test]
fn a_take_crossing_at_half_the_block_still_crosses_whole() {
    let profile = scaling_profile();
    let (mut writer, mut reader) = take_handoff(profile);
    let buffer = full_loop(profile);

    cross_in_blocks_of(&mut writer, &buffer, 4);

    let crossed = reader.claim().expect("the take crossed");
    assert_eq!(samples(&crossed), ramp(profile.max_loop_frames()));
}

#[test]
fn a_block_of_no_frames_carries_none_of_the_take_across() {
    let profile = scaling_profile();
    let (mut writer, mut reader) = take_handoff(profile);
    let buffer = full_loop(profile);

    writer.begin(&buffer);
    for _ in 0..ADVANCES_ALLOWED {
        assert!(writer.advance(&buffer, 0), "the crossing gave up");
    }

    assert!(reader.claim().is_none());
}

#[test]
fn a_block_longer_than_the_profiles_crosses_no_faster_than_its_scratch() {
    let profile = scaling_profile();
    let (mut writer, _reader) = take_handoff(profile);
    let buffer = full_loop(profile);
    let whole = cross_in_blocks_of(&mut writer, &buffer, 8);

    let overlong = cross_in_blocks_of(&mut writer, &buffer, 64);

    assert_eq!(overlong, whole);
}

#[test]
fn crossing_a_take_allocates_nothing() {
    let (mut writer, _reader) = take_handoff(eight_frame_profile());
    let buffer = recorded(&[0.25, 0.5, 0.75, 1.0, 0.125, 0.25, 0.5, 0.75]);

    let before = allocations();
    cross(&mut writer, &buffer);
    let after = allocations();

    assert_eq!(
        after, before,
        "the crossing allocated on the callback's end"
    );
}
