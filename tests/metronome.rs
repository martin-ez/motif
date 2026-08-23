//! The click on the beat, and where in the block it lands.
//!
//! The facts worth stating are that a beat inside a block sounds on the frame
//! it falls on rather than on the edge of the block, that a click too long for
//! one block carries into the next, that a beat outside the block sounds
//! nothing, that the click is summed over what is already playing and held
//! inside full scale, and that a block allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use motif::audio::{AudioPath, Command, Metronome, Passthrough, StreamConfig, sample_clock};
use motif::seq::{BeatGrid, ScheduleReader, beat_schedule};

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
const BLOCK: usize = 16;

/// Far enough past a beat that no test's block reaches the one after it.
const A_LONG_WAY: u64 = 1_000;

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: SAMPLE_RATE,
        block_size: BLOCK as u32,
        input_channels: 1,
        output_channels: 1,
    }
}

/// A schedule holding `beat` and `next`, and no other beat inside a block.
fn scheduled_pair(beat: u64, next: u64) -> ScheduleReader {
    let (mut writer, reader) = beat_schedule();
    let mut grid = BeatGrid::new(SAMPLE_RATE);
    assert!(grid.push(beat));
    assert!(grid.push(next));
    writer.follow(&grid, beat - 1);

    reader
}

/// A schedule whose first beat is `beat`, and whose next is a long way after.
fn scheduled(beat: u64) -> ScheduleReader {
    scheduled_pair(beat, beat + A_LONG_WAY)
}

/// A path that answers every command and records having been prepared.
struct Answering(Arc<AtomicBool>);

impl AudioPath for Answering {
    fn prepare(&mut self, _config: StreamConfig) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, _command: Command) -> bool {
        true
    }
}

#[test]
fn a_metronome_with_nothing_scheduled_plays_what_the_path_under_it_plays() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(beat_schedule().1, elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut playing = [0.0; BLOCK];
    metronome.render(&[0.25; BLOCK], &mut playing);

    assert_eq!(playing, [0.25; BLOCK]);
}

#[test]
fn a_click_starts_on_the_frame_its_beat_falls_on() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(5), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut playing = [0.0; BLOCK];
    metronome.render(&[0.0; BLOCK], &mut playing);

    assert_eq!(playing[..5], [0.0; 5], "the click started before its beat");
    assert_ne!(playing[5], 0.0, "the click did not start on its beat");
}

#[test]
fn a_click_carries_on_into_the_next_block() {
    let (mut frames, elapsed) = sample_clock(SAMPLE_RATE);
    let last = BLOCK as u64 - 1;
    let mut metronome = Metronome::over(scheduled(last), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut opening = [0.0; BLOCK];
    metronome.render(&[0.0; BLOCK], &mut opening);
    frames.advance(BLOCK);
    let mut following = [0.0; BLOCK];
    metronome.render(&[0.0; BLOCK], &mut following);

    assert_ne!(opening[BLOCK - 1], 0.0, "the click did not start");
    assert_ne!(following[0], 0.0, "the click stopped at the block edge");
}

#[test]
fn a_beat_the_block_has_not_reached_sounds_nothing() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(100), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut playing = [0.0; BLOCK];
    metronome.render(&[0.0; BLOCK], &mut playing);

    assert_eq!(playing, [0.0; BLOCK]);
}

#[test]
fn a_beat_the_block_has_passed_sounds_nothing() {
    let (mut frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(5), elapsed, Passthrough::new());
    metronome.prepare(config());
    frames.advance(BLOCK);

    let mut playing = [0.0; BLOCK];
    metronome.render(&[0.0; BLOCK], &mut playing);

    assert_eq!(playing, [0.0; BLOCK]);
}

#[test]
fn two_beats_in_one_block_each_sound() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut once = Metronome::over(scheduled(2), elapsed.clone(), Passthrough::new());
    let mut twice = Metronome::over(scheduled_pair(2, 9), elapsed, Passthrough::new());
    once.prepare(config());
    twice.prepare(config());

    let mut one = [0.0; BLOCK];
    let mut two = [0.0; BLOCK];
    once.render(&[0.0; BLOCK], &mut one);
    twice.render(&[0.0; BLOCK], &mut two);

    assert_eq!(one[..9], two[..9], "the second beat changed the first click");
    assert_ne!(one[9..], two[9..], "the second beat sounded nothing");
}

#[test]
fn the_click_is_summed_over_what_the_path_plays() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(5), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut playing = [0.0; BLOCK];
    metronome.render(&[0.25; BLOCK], &mut playing);

    assert_eq!(playing[..5], [0.25; 5]);
    assert_ne!(playing[5], 0.25, "the click replaced what was playing");
}

#[test]
fn the_sum_stays_inside_full_scale() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(1), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut playing = [0.0; BLOCK];
    metronome.render(&[1.0; BLOCK], &mut playing);

    for played in playing {
        assert!(played.abs() <= 1.0, "{played} is outside full scale");
    }
}

#[test]
fn a_metronome_prepares_the_path_under_it() {
    let prepared = Arc::new(AtomicBool::new(false));
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome =
        Metronome::over(beat_schedule().1, elapsed, Answering(Arc::clone(&prepared)));

    metronome.prepare(config());

    assert!(prepared.load(Ordering::Relaxed));
}

#[test]
fn a_metronome_answers_what_the_path_under_it_answers() {
    let (_frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut answering = Metronome::over(
        beat_schedule().1,
        elapsed.clone(),
        Answering(Arc::new(AtomicBool::new(false))),
    );
    let mut silent = Metronome::over(beat_schedule().1, elapsed, Passthrough::new());

    assert!(answering.apply(Command::Undo));
    assert!(!silent.apply(Command::Undo));
}

#[test]
fn a_block_does_not_allocate() {
    let (mut frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(5), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut playing = [0.0; BLOCK];
    let before = allocations();
    for _ in 0..BLOCK {
        metronome.render(black_box(&[0.25; BLOCK]), black_box(&mut playing));
        frames.advance(BLOCK);
    }
    let after = allocations();

    assert_eq!(after, before, "a block allocated");
}
