//! The beats on their way to the callback, and how they get there.
//!
//! The facts worth stating are that a schedule carries the beats a grid
//! projects ahead of a frame, that following again replaces what was there
//! rather than adding to it, that a grid stating no tempo schedules nothing,
//! and that the end which reads runs on the audio thread, so reading allocates
//! nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use motif::seq::{BeatGrid, BeatsAhead, beat_schedule};

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
/// Zeroed allocation is counted alongside plain allocation, so a growth asking
/// for pre-zeroed storage is seen as the allocation it is.
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

const SAMPLE_RATE: u32 = 48_000;

/// Half a second at [`SAMPLE_RATE`], which is 120 BPM.
const HALF_SECOND: u64 = 24_000;

fn grid_of(beats: &[u64]) -> BeatGrid {
    let mut grid = BeatGrid::new(SAMPLE_RATE);
    for &beat in beats {
        assert!(grid.push(beat), "{beat} comes after the beat before it");
    }

    grid
}

fn three_beats_at_120() -> BeatGrid {
    grid_of(&[0, HALF_SECOND, 2 * HALF_SECOND])
}

#[test]
fn a_schedule_nobody_has_followed_holds_no_beats() {
    let (_writer, reader) = beat_schedule();

    assert_eq!(reader.read().beats(), &[]);
}

#[test]
fn the_beats_a_grid_projects_are_the_beats_read() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&three_beats_at_120(), 0);

    assert_eq!(
        reader.read().beats(),
        &[
            HALF_SECOND,
            2 * HALF_SECOND,
            3 * HALF_SECOND,
            4 * HALF_SECOND,
            5 * HALF_SECOND,
            6 * HALF_SECOND,
            7 * HALF_SECOND,
            8 * HALF_SECOND,
        ]
    );
}

#[test]
fn a_schedule_starts_after_the_frame_it_was_given() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&three_beats_at_120(), 2 * HALF_SECOND);

    assert_eq!(reader.read().beats()[0], 3 * HALF_SECOND);
}

#[test]
fn a_schedule_holds_as_many_beats_as_it_reaches_ahead() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&three_beats_at_120(), 0);

    assert_eq!(reader.read().beats().len(), BeatsAhead::BEATS);
}

#[test]
fn following_again_replaces_the_beats_that_were_there() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&three_beats_at_120(), 0);
    writer.follow(&grid_of(&[0, HALF_SECOND / 2, HALF_SECOND]), 0);

    assert_eq!(reader.read().beats()[0], HALF_SECOND / 2);
}

#[test]
fn a_grid_with_no_interval_schedules_nothing_ahead() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&grid_of(&[HALF_SECOND]), HALF_SECOND);

    assert_eq!(reader.read().beats(), &[]);
}

#[test]
fn a_projection_that_outruns_the_clock_ends_the_schedule() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&three_beats_at_120(), u64::MAX);

    assert_eq!(reader.read().beats(), &[]);
}

#[test]
fn silence_takes_the_beats_off_the_schedule() {
    let (mut writer, reader) = beat_schedule();

    writer.follow(&three_beats_at_120(), 0);
    writer.silence();

    assert_eq!(reader.read().beats(), &[]);
}

#[test]
fn reading_a_schedule_does_not_allocate() {
    let (mut writer, reader) = beat_schedule();
    writer.follow(&three_beats_at_120(), 0);

    let before = allocations();
    for _ in 0..BeatsAhead::BEATS {
        black_box(black_box(&reader).read());
    }
    let after = allocations();

    assert_eq!(after, before, "reading a schedule allocated");
}
