//! The frame count the callback keeps, and the thread that stamps a tap with
//! it.
//!
//! The facts worth stating are that the count starts at nothing, that it
//! accumulates block by block, that it carries the rate those frames are
//! counted at, that reading it takes nothing away, that a reader on another
//! thread never sees it go backwards, and that advancing it allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::sample_clock;

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

const BLOCK: usize = 128;

/// A rate no device profile in the crate uses, so a reading of it can only have
/// come from the clock it was made with.
const SAMPLE_RATE: u32 = 44_100;

/// How long the two-thread test spends looking for a count that went backwards.
const SEARCH: Duration = Duration::from_millis(50);

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    std::hint::black_box(Vec::<u64>::with_capacity(4));
    let after = allocations();

    assert!(after > before, "the counter is not wired to the allocator");
}

#[test]
fn a_new_clock_has_counted_no_frames() {
    let (_writer, reader) = sample_clock(SAMPLE_RATE);

    assert_eq!(reader.read(), 0);
}

#[test]
fn a_clock_carries_the_rate_its_frames_are_counted_at() {
    let (_writer, reader) = sample_clock(SAMPLE_RATE);

    assert_eq!(reader.sample_rate(), SAMPLE_RATE);
}

#[test]
fn a_block_moves_the_clock_on_by_its_frames() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);

    writer.advance(BLOCK);

    assert_eq!(reader.read(), BLOCK as u64);
}

#[test]
fn blocks_accumulate() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);

    for _ in 0..4 {
        writer.advance(BLOCK);
    }

    assert_eq!(reader.read(), 4 * BLOCK as u64);
}

#[test]
fn advancing_reports_the_count_it_reached() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);
    writer.advance(BLOCK);

    let reached = writer.advance(BLOCK);

    assert_eq!(reached, reader.read());
}

#[test]
fn a_block_of_no_frames_leaves_the_clock_where_it_is() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);
    writer.advance(BLOCK);

    writer.advance(0);

    assert_eq!(reader.read(), BLOCK as u64);
}

#[test]
fn reading_leaves_the_clock_where_it_is() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);
    writer.advance(BLOCK);

    let read = reader.read();

    assert_eq!(reader.read(), read);
}

#[test]
fn a_reader_never_sees_the_clock_go_backwards() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);
    let stop = Arc::new(AtomicBool::new(false));
    let advancing = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                writer.advance(BLOCK);
            }
        })
    };

    let until = Instant::now() + SEARCH;
    let mut last = reader.read();
    while Instant::now() < until {
        let now = reader.read();
        assert!(now >= last, "the clock went back from {last} to {now}");
        last = now;
    }

    stop.store(true, Ordering::Relaxed);
    advancing.join().expect("the advancing thread finishes");
}

#[test]
fn advancing_does_not_allocate() {
    let (mut writer, reader) = sample_clock(SAMPLE_RATE);

    let before = allocations();
    for _ in 0..8 {
        writer.advance(BLOCK);
    }
    let after = allocations();

    assert_eq!(after, before, "advancing the clock allocated");
    assert_eq!(reader.read(), 8 * BLOCK as u64);
}
