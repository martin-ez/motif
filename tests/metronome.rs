//! The click on the beat, and where in the block it lands.
//!
//! The facts worth stating are that a beat inside a block sounds on the frame
//! it falls on rather than on the edge of the block, that a click too long for
//! one block carries into the next, that a beat outside the block sounds
//! nothing, that the click is summed over what is already playing and held
//! inside full scale, and that a block allocates nothing.
//!
//! What the click sounds like is stated here too — how long it lasts, that it
//! decays, what it swings at and that it leaves room above itself — because
//! none of that is visible in an assertion that a sample is merely not silent.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use motif::audio::{
    AudioPath, Command, HELD_ABOVE, Metronome, Passthrough, StreamConfig, sample_clock,
};
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

/// Eight milliseconds at [`SAMPLE_RATE`], which is how long a click sounds for.
const CLICK_FRAMES: usize = SAMPLE_RATE as usize * 8 / 1_000;

/// Half a cycle of the kilohertz a click swings at, where a cosine is at its
/// most negative.
const HALF_A_CYCLE: usize = SAMPLE_RATE as usize / 1_000 / 2;

/// The frame the one beat of a sounded run falls on.
const FIRST_BEAT: usize = 1;

/// Enough blocks to carry a whole click and leave silence after it.
const BLOCKS_PAST_A_CLICK: usize = (FIRST_BEAT + CLICK_FRAMES) / BLOCK + 2;

/// A level to play under a click, low enough to leave the sum inside scale.
const UNDER: f32 = 0.25;

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

/// Every frame a metronome plays over [`BLOCKS_PAST_A_CLICK`] blocks, with one
/// beat at `beat` and `under` playing beneath it.
fn sounded_from(beat: u64, under: f32) -> Vec<f32> {
    let (mut frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut metronome = Metronome::over(scheduled(beat), elapsed, Passthrough::new());
    metronome.prepare(config());

    let mut sounded = Vec::new();
    for _ in 0..BLOCKS_PAST_A_CLICK {
        let mut playing = [0.0; BLOCK];
        metronome.render(&[under; BLOCK], &mut playing);
        frames.advance(BLOCK);
        sounded.extend_from_slice(&playing);
    }

    sounded
}

fn sounded_past_a_click(under: f32) -> Vec<f32> {
    sounded_from(FIRST_BEAT as u64, under)
}

fn just_the_click(sounded: &[f32]) -> &[f32] {
    &sounded[FIRST_BEAT..FIRST_BEAT + CLICK_FRAMES]
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |top: f32, played| top.max(played.abs()))
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

    assert_eq!(
        one[..9],
        two[..9],
        "the second beat changed the first click"
    );
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

#[test]
fn a_click_sounds_for_eight_milliseconds_and_then_stops() {
    let sounded = sounded_past_a_click(0.0);

    assert_eq!(
        sounded.iter().rposition(|played| *played != 0.0),
        Some(FIRST_BEAT + CLICK_FRAMES - 1)
    );
}

#[test]
fn a_click_is_loudest_where_it_starts() {
    let sounded = sounded_past_a_click(0.0);
    let (opening, closing) = just_the_click(&sounded).split_at(CLICK_FRAMES / 2);

    assert!(peak(opening) > peak(closing), "the click did not decay");
}

#[test]
fn a_click_leaves_room_above_it_for_what_it_is_played_over() {
    let sounded = sounded_past_a_click(0.0);

    assert!(peak(just_the_click(&sounded)) < HELD_ABOVE);
}

#[test]
fn a_click_swings_at_a_kilohertz() {
    let sounded = sounded_past_a_click(0.0);
    let trough = just_the_click(&sounded)
        .iter()
        .enumerate()
        .min_by(|one, two| one.1.total_cmp(two.1))
        .map(|(at, _)| at);

    assert_eq!(trough, Some(HALF_A_CYCLE));
}

#[test]
fn a_click_adds_to_what_is_already_playing() {
    let sounded = sounded_past_a_click(UNDER);

    assert!(
        sounded[FIRST_BEAT] > UNDER,
        "the click was taken off what was playing"
    );
}

#[test]
fn a_beat_on_the_edge_of_a_block_sounds_once() {
    let inside = sounded_past_a_click(0.0);
    let on_the_edge = sounded_from(BLOCK as u64, 0.0);

    assert_eq!(
        on_the_edge[BLOCK], inside[FIRST_BEAT],
        "a beat on the edge sounded in the block either side of it"
    );
}
