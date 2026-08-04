//! The grid of beat timestamps, and the tempo read off it.
//!
//! The facts worth stating are that the beats come back as they went in, that
//! a timestamp out of order is refused, that a frame is placed against the
//! beats around it, that the tempo follows the beats rather than being kept
//! beside them, and that reading a grid allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use motif::seq::{BeatGrid, Position};

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
/// Zeroed allocation is counted alongside plain allocation, so that a growth
/// asking for pre-zeroed storage is seen as the allocation it is.
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

fn four_beats_at_120() -> BeatGrid {
    grid_of(&[0, HALF_SECOND, 2 * HALF_SECOND, 3 * HALF_SECOND])
}

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    black_box(Vec::<u64>::with_capacity(4));
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
fn a_new_grid_has_no_beats() {
    let grid = BeatGrid::new(SAMPLE_RATE);

    assert!(grid.is_empty());
    assert_eq!(grid.len(), 0);
    assert_eq!(grid.beats(), &[]);
}

#[test]
fn a_grid_keeps_the_sample_rate_its_beats_were_timed_against() {
    let grid = BeatGrid::new(SAMPLE_RATE);

    assert_eq!(grid.sample_rate(), SAMPLE_RATE);
}

#[test]
fn beats_come_back_in_the_order_they_were_added() {
    let grid = four_beats_at_120();

    assert_eq!(grid.len(), 4);
    assert_eq!(grid.beats(), &[0, 24_000, 48_000, 72_000]);
}

#[test]
fn a_beat_that_does_not_come_after_the_last_is_refused() {
    let mut grid = grid_of(&[HALF_SECOND]);

    assert!(!grid.push(HALF_SECOND - 1));
    assert!(!grid.push(HALF_SECOND));
    assert_eq!(grid.beats(), &[HALF_SECOND]);
}

#[test]
fn a_tempo_needs_two_beats() {
    assert_eq!(BeatGrid::new(SAMPLE_RATE).beats_per_minute(), None);
    assert_eq!(grid_of(&[HALF_SECOND]).beats_per_minute(), None);
}

#[test]
fn two_beats_are_enough_for_a_tempo() {
    let grid = grid_of(&[0, HALF_SECOND]);

    assert_eq!(grid.beats_per_minute(), Some(120.0));
}

#[test]
fn tempo_is_derived_from_the_beats() {
    let grid = four_beats_at_120();

    assert_eq!(grid.beats_per_minute(), Some(120.0));
}

#[test]
fn a_grid_that_slows_down_reports_a_slower_tempo() {
    let mut grid = four_beats_at_120();

    assert!(grid.push(3 * HALF_SECOND + 2 * HALF_SECOND));

    assert_eq!(grid.beats_per_minute(), Some(96.0));
}

#[test]
fn a_grid_with_no_sample_rate_has_no_tempo() {
    let mut grid = BeatGrid::new(0);
    assert!(grid.push(0));
    assert!(grid.push(HALF_SECOND));

    assert_eq!(grid.beats_per_minute(), None);
}

#[test]
fn an_empty_grid_places_every_frame_before_its_first_beat() {
    let grid = BeatGrid::new(SAMPLE_RATE);

    assert_eq!(grid.position(0), Position::BeforeFirst);
    assert_eq!(grid.position(u64::MAX), Position::BeforeFirst);
}

#[test]
fn a_frame_before_the_first_beat_is_before_the_grid() {
    let grid = grid_of(&[HALF_SECOND, 2 * HALF_SECOND]);

    assert_eq!(grid.position(HALF_SECOND - 1), Position::BeforeFirst);
}

#[test]
fn a_frame_on_a_beat_is_at_the_start_of_it() {
    let grid = four_beats_at_120();

    assert_eq!(
        grid.position(HALF_SECOND),
        Position::Within {
            beat: 1,
            phase: 0.0
        }
    );
}

#[test]
fn a_frame_between_two_beats_reports_how_far_through_it_is() {
    let grid = four_beats_at_120();

    assert_eq!(
        grid.position(HALF_SECOND + HALF_SECOND / 4),
        Position::Within {
            beat: 1,
            phase: 0.25
        }
    );
}

#[test]
fn phase_is_measured_against_the_beats_a_frame_falls_between() {
    let grid = grid_of(&[0, HALF_SECOND, HALF_SECOND + 36_000]);

    assert_eq!(
        grid.position(HALF_SECOND + 9_000),
        Position::Within {
            beat: 1,
            phase: 0.25
        }
    );
}

#[test]
fn a_frame_past_the_last_beat_is_on_the_last_beat() {
    let grid = four_beats_at_120();

    assert_eq!(
        grid.position(3 * HALF_SECOND),
        Position::AfterLast { beat: 3 }
    );
    assert_eq!(grid.position(u64::MAX), Position::AfterLast { beat: 3 });
}

#[test]
fn reading_a_grid_does_not_allocate() {
    let grid = four_beats_at_120();

    let before = allocations();
    for frame in 0..4 * HALF_SECOND {
        black_box(grid.position(black_box(frame)));
    }
    black_box(grid.beats_per_minute());
    black_box(grid.beats());
    let after = allocations();

    assert_eq!(after, before, "reading a grid allocated");
}
