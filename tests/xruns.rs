//! Dropouts counted in the audio callback and read from another thread.
//!
//! The facts worth stating are the rule that decides an xrun — a path that
//! handled less than it was given — that the two directions are told apart,
//! that reading is a look rather than a take, and that counting allocates
//! nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::thread;

use motif::audio::{
    AudioBackend, DuplexStream, NullBackend, StreamConfig, StreamRequest, Xruns, xrun_counter,
};

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

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

/// The frames a callback is given, standing in for a device's block size.
const BLOCK: usize = 64;

const GRANTED: StreamConfig = StreamConfig {
    sample_rate: 48_000,
    block_size: BLOCK as u32,
    input_channels: 1,
    output_channels: 2,
};

#[test]
fn a_counter_starts_with_nothing_counted() {
    let (_overruns, _underruns, reader) = xrun_counter();

    assert_eq!(reader.read(), Xruns::NONE);
}

#[test]
fn a_block_captured_whole_is_not_counted() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(BLOCK, BLOCK);

    assert_eq!(reader.read(), Xruns::NONE);
}

#[test]
fn a_block_captured_short_of_its_frames_is_counted() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(BLOCK - 1, BLOCK);

    assert_eq!(reader.read().overruns, 1);
}

#[test]
fn a_block_captured_not_at_all_is_counted() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(0, BLOCK);

    assert_eq!(reader.read().overruns, 1);
}

#[test]
fn a_block_supplied_whole_is_not_counted() {
    let (_overruns, mut underruns, reader) = xrun_counter();

    underruns.supplied(BLOCK, BLOCK);

    assert_eq!(reader.read(), Xruns::NONE);
}

#[test]
fn a_block_supplied_short_of_its_frames_is_counted() {
    let (_overruns, mut underruns, reader) = xrun_counter();

    underruns.supplied(BLOCK - 1, BLOCK);

    assert_eq!(reader.read().underruns, 1);
}

#[test]
fn a_block_supplied_not_at_all_is_counted() {
    let (_overruns, mut underruns, reader) = xrun_counter();

    underruns.supplied(0, BLOCK);

    assert_eq!(reader.read().underruns, 1);
}

#[test]
fn a_shortfall_is_the_only_thing_counted() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    overruns.captured(BLOCK + 1, BLOCK);
    underruns.supplied(BLOCK + 1, BLOCK);

    assert_eq!(reader.read(), Xruns::NONE);
}

#[test]
fn a_callback_that_lost_frames_counts_once_however_many_it_lost() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(1, BLOCK);

    assert_eq!(reader.read().overruns, 1);
}

#[test]
fn an_overrun_is_counted_on_its_own() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(0, BLOCK);

    assert_eq!(
        reader.read(),
        Xruns {
            overruns: 1,
            underruns: 0
        }
    );
}

#[test]
fn an_underrun_is_counted_on_its_own() {
    let (_overruns, mut underruns, reader) = xrun_counter();

    underruns.supplied(0, BLOCK);

    assert_eq!(
        reader.read(),
        Xruns {
            overruns: 0,
            underruns: 1
        }
    );
}

#[test]
fn each_direction_counts_only_its_own_dropouts() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    for _ in 0..3 {
        overruns.captured(0, BLOCK);
    }
    for _ in 0..5 {
        underruns.supplied(0, BLOCK);
    }

    assert_eq!(
        reader.read(),
        Xruns {
            overruns: 3,
            underruns: 5
        }
    );
}

#[test]
fn counts_accumulate_rather_than_replace() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(0, BLOCK);
    let after_one = reader.read();
    overruns.captured(0, BLOCK);

    assert_eq!(after_one.overruns, 1);
    assert_eq!(reader.read().overruns, 2);
}

#[test]
fn a_callback_that_lost_nothing_leaves_the_count_where_it_was() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.captured(0, BLOCK);
    overruns.captured(BLOCK, BLOCK);

    assert_eq!(reader.read().overruns, 1);
}

#[test]
fn reading_does_not_reset_the_count() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    overruns.captured(0, BLOCK);
    underruns.supplied(0, BLOCK);

    assert_eq!(reader.read(), reader.read());
}

#[test]
fn a_count_crosses_from_the_thread_that_made_it() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    let counting = thread::spawn(move || {
        for _ in 0..BLOCK {
            overruns.captured(0, BLOCK);
            underruns.supplied(0, BLOCK);
        }
    });
    counting.join().expect("the counting thread finishes");

    assert_eq!(
        reader.read(),
        Xruns {
            overruns: BLOCK,
            underruns: BLOCK
        }
    );
}

#[test]
fn counting_does_not_allocate() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    let before = allocations();
    for _ in 0..128 {
        overruns.captured(0, BLOCK);
        underruns.supplied(0, BLOCK);
    }
    let after = allocations();

    assert_eq!(after, before);
    assert_eq!(
        reader.read(),
        Xruns {
            overruns: 128,
            underruns: 128
        }
    );
}

#[test]
fn reading_does_not_allocate() {
    let (mut overruns, _underruns, reader) = xrun_counter();
    overruns.captured(0, BLOCK);

    let before = allocations();
    for _ in 0..128 {
        reader.read();
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn a_stream_that_moves_no_samples_reports_no_xruns() {
    let stream = NullBackend::rounding(GRANTED)
        .open(StreamRequest {
            sample_rate: GRANTED.sample_rate,
            block_size: GRANTED.block_size,
        })
        .expect("a rounding backend opens whatever it is asked for");

    assert_eq!(stream.xruns(), Xruns::NONE);
}
