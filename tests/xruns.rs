//! Dropouts counted in the audio callback and read from the application
//! thread.
//!
//! An xrun is a block that was lost, and the facts worth stating are that the
//! two directions are told apart, that reading is a look rather than a take,
//! and that counting one allocates nothing — a counter that broke the
//! real-time invariant to report it breaking would be worse than no counter.

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

const GRANTED: StreamConfig = StreamConfig {
    sample_rate: 48_000,
    block_size: 64,
    input_channels: 1,
    output_channels: 2,
};

#[test]
fn a_counter_starts_with_nothing_counted() {
    let (_overruns, _underruns, reader) = xrun_counter();

    assert_eq!(reader.read(), Xruns::NONE);
}

#[test]
fn an_overrun_is_counted_on_its_own() {
    let (mut overruns, _underruns, reader) = xrun_counter();

    overruns.overran();

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

    underruns.underran();

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
        overruns.overran();
    }
    for _ in 0..5 {
        underruns.underran();
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

    overruns.overran();
    let after_one = reader.read();
    overruns.overran();

    assert_eq!(after_one.overruns, 1);
    assert_eq!(reader.read().overruns, 2);
}

#[test]
fn reading_does_not_reset_the_count() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    overruns.overran();
    underruns.underran();

    assert_eq!(reader.read(), reader.read());
}

#[test]
fn a_count_crosses_from_the_thread_that_made_it() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    let counting = thread::spawn(move || {
        for _ in 0..64 {
            overruns.overran();
            underruns.underran();
        }
    });
    counting.join().expect("the counting thread finishes");

    assert_eq!(
        reader.read(),
        Xruns {
            overruns: 64,
            underruns: 64
        }
    );
}

#[test]
fn counting_does_not_allocate() {
    let (mut overruns, mut underruns, reader) = xrun_counter();

    let before = allocations();
    for _ in 0..128 {
        overruns.overran();
        underruns.underran();
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
    overruns.overran();

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
