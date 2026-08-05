//! The fixed store a captured loop lands in.
//!
//! Its capacity comes from the device profile and is decided before the stream
//! starts, so the facts worth stating are that the buffer is exactly as long as
//! the profile says, that frames come back as they went in, and that a recording
//! longer than the buffer is reported short rather than growing it.
//!
//! Layers are the other half, and a loop is heard as their sum, so the tests
//! state what is heard: an overdub lies over the take without lengthening it,
//! undo leaves the rest playing, the stack stops at a stated depth, and clear
//! empties the loop.
//!
//! Playing is where the boundary matters: the tests state where the playhead
//! lands, and that blocks of it tile the loop exactly. All of it runs on the
//! thread that may not allocate, so the allocations are counted too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use motif::device::{AudioProfile, DeviceProfile};
use motif::looper::{Extremes, LoopBuffer, LoopWaveform};

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
/// Zeroed allocation is counted alongside plain allocation: a loop buffer is a
/// block of silence, which [`GlobalAlloc::alloc`] alone would miss.
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
/// The sample values below are binary fractions, so summing them is exact and
/// so is comparing against the result.
fn heard(buffer: &LoopBuffer) -> Vec<f32> {
    let mut block = vec![0.0; buffer.len()];
    buffer.mix_into(&mut block, 0);

    block
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
fn an_overdub_over_an_empty_buffer_is_refused() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    assert!(!buffer.overdub());
    assert_eq!(buffer.depth(), 0);
}

#[test]
fn an_empty_buffer_that_refused_an_overdub_still_takes_a_take() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.overdub();

    let recorded = buffer.record(&[0.25, 0.5]);

    assert_eq!(recorded, 2);
    assert_eq!(heard(&buffer), [0.25, 0.5]);
}

#[test]
fn recording_after_an_undo_takes_nothing_until_a_layer_is_opened() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);
    buffer.undo();

    let recorded = buffer.record(&[0.75]);

    assert_eq!(recorded, 0);
    assert_eq!(buffer.len(), 2);
    assert_eq!(heard(&buffer), [0.25, 0.5]);
}

#[test]
fn a_refused_overdub_leaves_the_top_layer_open() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    for _ in 1..LoopBuffer::LAYERS {
        buffer.overdub();
    }
    buffer.record(&[0.125]);

    buffer.overdub();
    let recorded = buffer.record(&[0.125]);

    assert_eq!(recorded, 1);
    assert_eq!(heard(&buffer), [0.375, 0.625]);
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
    let mut block = [0.0; 4];

    let mixed = buffer.mix_into(&mut block, 0);

    assert_eq!(mixed, 2);
    assert_eq!(block, [0.25, 0.5, 0.0, 0.0]);
}

#[test]
fn mixing_adds_to_what_the_block_already_holds() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    let mut block = [1.0; 3];

    buffer.mix_into(&mut block, 0);

    assert_eq!(block, [1.25, 1.5, 1.0]);
}

#[test]
fn mixing_reads_on_past_a_layer_that_ended_early() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    buffer.overdub();
    buffer.record(&[0.125]);
    let mut block = [0.0; 2];

    let mixed = buffer.mix_into(&mut block, 1);

    assert_eq!(mixed, 2);
    assert_eq!(block, [0.5, 0.75]);
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
fn playing_wraps_at_the_loop_boundary_inside_a_block() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    let mut block = [0.0; 5];

    buffer.play_into(&mut block, 0);

    assert_eq!(block, [0.25, 0.5, 0.75, 0.25, 0.5]);
}

#[test]
fn playing_reports_the_playhead_the_block_ended_on() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    let mut block = [0.0; 5];

    assert_eq!(buffer.play_into(&mut block, 0), 2);
}

#[test]
fn a_block_ending_on_the_loop_boundary_reports_the_start_of_the_loop() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    let mut block = [0.0; 3];

    assert_eq!(buffer.play_into(&mut block, 0), 0);
}

#[test]
fn a_loop_shorter_than_the_block_is_heard_as_often_as_it_fits() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    let mut block = [0.0; 5];

    buffer.play_into(&mut block, 0);

    assert_eq!(block, [0.25, 0.5, 0.25, 0.5, 0.25]);
}

#[test]
fn playing_from_past_the_end_of_the_loop_starts_at_its_beginning() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    let mut block = [0.0; 2];

    let playhead = buffer.play_into(&mut block, 7);

    assert_eq!(block, [0.25, 0.5]);
    assert_eq!(playhead, 2);
}

#[test]
fn a_playhead_kept_across_a_shorter_take_does_not_leave_it_out_of_phase() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75, 0.125, 0.375]);
    let mut block = [0.0; 3];
    let kept = buffer.play_into(&mut block, 0);

    buffer.clear();
    buffer.record(&[0.25, 0.5]);
    let mut played = [0.0; 2];
    buffer.play_into(&mut played, kept);

    assert_eq!(played, [0.25, 0.5]);
}

#[test]
fn playing_an_empty_loop_leaves_the_block_as_it_was() {
    let buffer = LoopBuffer::for_profile(eight_frame_profile());
    let mut block = [9.0; 2];

    let playhead = buffer.play_into(&mut block, 0);

    assert_eq!(block, [9.0, 9.0]);
    assert_eq!(playhead, 0);
}

#[test]
fn playing_adds_to_what_the_block_already_holds() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    let mut block = [1.0; 3];

    buffer.play_into(&mut block, 0);

    assert_eq!(block, [1.25, 1.5, 1.25]);
}

#[test]
fn a_layer_shorter_than_the_loop_keeps_its_place_across_the_wrap() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75]);
    buffer.overdub();
    buffer.record(&[0.125]);
    let mut block = [0.0; 5];

    buffer.play_into(&mut block, 0);

    assert_eq!(block, [0.375, 0.5, 0.75, 0.375, 0.5]);
}

#[test]
fn a_loop_that_is_not_a_multiple_of_the_block_repeats_without_drift() {
    let recorded = [0.25, 0.5, 0.75, 0.125, 0.375];
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&recorded);
    let blocks = 10;
    let mut played = Vec::new();
    let mut playhead = 0;

    for _ in 0..blocks {
        let mut block = [0.0; 4];
        playhead = buffer.play_into(&mut block, playhead);
        played.extend_from_slice(&block);
    }

    let tiled: Vec<f32> = recorded.iter().copied().cycle().take(blocks * 4).collect();
    assert_eq!(played, tiled);
}

#[test]
fn playing_a_stack_of_layers_across_the_wrap_does_not_allocate() {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.5; profile.block_size as usize];
    let mut played = vec![0.0; profile.block_size as usize];
    buffer.record(&block[..block.len() - 1]);
    for _ in 1..LoopBuffer::LAYERS {
        buffer.overdub();
        buffer.record(&block);
    }

    let before = allocations();
    let mut playhead = 0;
    for _ in 0..LoopBuffer::LAYERS {
        playhead = buffer.play_into(&mut played, playhead);
    }
    let after = allocations();

    assert_eq!(after, before);
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
    for _ in 1..LoopBuffer::LAYERS {
        buffer.undo();
        buffer.mix_into(&mut mixed, 0);
    }
    buffer.clear();
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn a_take_shows_in_the_waveform_as_it_is_recorded() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());

    buffer.record(&[0.25, -0.5]);

    assert_eq!(
        buffer.waveform().buckets(),
        [
            Extremes {
                peak: 0.25,
                trough: 0.0
            },
            Extremes {
                peak: 0.0,
                trough: -0.5
            },
        ]
    );
}

#[test]
fn a_cleared_loop_has_no_waveform() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    buffer.clear();

    assert!(buffer.waveform().buckets().is_empty());
}

#[test]
fn the_waveform_of_a_stack_of_layers_is_their_sum() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    buffer.overdub();
    buffer.record(&[0.125, 0.125]);

    assert_eq!(buffer.waveform().buckets()[0].peak, 0.375);
    assert_eq!(buffer.waveform().buckets()[1].peak, 0.625);
}

#[test]
fn an_overdub_repaints_only_as_far_as_it_has_reached() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    buffer.overdub();
    buffer.record(&[0.125]);

    assert_eq!(buffer.waveform().buckets()[0].peak, 0.375);
    assert_eq!(buffer.waveform().buckets()[1].peak, 0.5);
}

#[test]
fn a_take_longer_than_the_buckets_still_spans_the_loop() {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.5; profile.block_size as usize];
    for _ in 0..LoopWaveform::BUCKETS {
        buffer.record(&block);
    }

    let buckets = buffer.waveform().buckets();

    assert!(buckets.len() <= LoopWaveform::BUCKETS);
    assert_eq!(buckets.last().map(|bucket| bucket.peak), Some(0.5));
}

#[test]
fn a_resweep_leaves_a_waveform_of_the_layers_that_remain() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);
    buffer.undo();

    while buffer.resummarise(buffer.len()) {}

    assert_eq!(buffer.waveform().buckets()[0].peak, 0.25);
    assert_eq!(buffer.waveform().buckets()[1].peak, 0.5);
}

#[test]
fn an_undone_layer_stays_in_the_waveform_until_the_resweep_reaches_it() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75, 1.0]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125, 0.125, 0.125]);
    buffer.undo();

    buffer.resummarise(2);

    assert_eq!(buffer.waveform().buckets()[1].peak, 0.5);
    assert_eq!(buffer.waveform().buckets()[2].peak, 0.875);
}

#[test]
fn a_resweep_covers_no_more_than_the_frames_it_is_given() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75, 1.0]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125, 0.125, 0.125]);
    buffer.undo();

    assert!(buffer.resummarise(3));
    assert!(!buffer.resummarise(3));
    assert_eq!(buffer.waveform().buckets().len(), 4);
}

#[test]
fn a_resweep_of_the_whole_loop_reports_nothing_left_to_cover() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);
    buffer.undo();

    assert!(!buffer.resummarise(2));
}

#[test]
fn a_loop_with_nothing_undone_has_nothing_to_resweep() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);

    assert!(!buffer.resummarise(2));
    assert_eq!(buffer.waveform().buckets()[0].peak, 0.375);
}

#[test]
fn a_refused_undo_leaves_nothing_to_resweep() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);

    assert!(!buffer.undo());
    assert!(!buffer.resummarise(2));
}

#[test]
fn clearing_leaves_nothing_to_resweep() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125]);
    buffer.undo();

    buffer.clear();

    assert!(!buffer.resummarise(2));
    assert!(buffer.waveform().buckets().is_empty());
}

#[test]
fn a_second_undo_sends_the_resweep_back_to_the_start_of_the_loop() {
    let mut buffer = LoopBuffer::for_profile(eight_frame_profile());
    buffer.record(&[0.25, 0.5, 0.75, 1.0]);
    buffer.overdub();
    buffer.record(&[0.125, 0.125, 0.125, 0.125]);
    buffer.overdub();
    buffer.record(&[0.5, 0.5, 0.5, 0.5]);
    buffer.undo();
    buffer.resummarise(4);

    buffer.undo();
    while buffer.resummarise(1) {}

    assert_eq!(buffer.waveform().buckets()[0].peak, 0.25);
    assert_eq!(buffer.waveform().buckets()[3].peak, 1.0);
}

#[test]
fn resweeping_does_not_allocate() {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.5; profile.block_size as usize];
    for _ in 0..LoopWaveform::BUCKETS {
        buffer.record(&block);
    }
    buffer.overdub();
    buffer.record(&block);
    buffer.undo();

    let before = allocations();
    while buffer.resummarise(block.len()) {}
    let after = allocations();

    assert_eq!(after, before);
}
