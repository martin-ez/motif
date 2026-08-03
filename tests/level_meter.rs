//! Peak and RMS levels crossing from the audio callback to the application
//! thread.
//!
//! The callback measures a block and publishes it; the application thread reads
//! whatever was published last. The facts worth stating are what each number
//! means, that a reader sees a pair from one block rather than halves of two,
//! and that publishing allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::{Levels, level_meter};

/// How long the reader keeps checking that it never sees half of one block
/// beside half of another. It bounds a search for a race rather than a wait for
/// one, so a slow machine only reads more times, and the test takes about this
/// long either way.
const SEARCH: Duration = Duration::from_millis(50);

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

fn assert_near(measured: f32, expected: f32) {
    assert!(
        (measured - expected).abs() < 1e-6,
        "{measured} is not {expected}"
    );
}

fn sine(cycles: usize, samples: usize) -> Vec<f32> {
    let turn = std::f32::consts::TAU * cycles as f32 / samples as f32;
    (0..samples).map(|n| (turn * n as f32).sin()).collect()
}

#[test]
fn peak_is_the_largest_absolute_sample_in_the_block() {
    let levels = Levels::of(&[0.2, -0.8, 0.5]);

    assert_eq!(levels.peak, 0.8);
}

#[test]
fn rms_is_the_root_mean_square_of_the_block() {
    let levels = Levels::of(&[1.0, 0.0, -1.0, 0.0]);

    assert_near(levels.rms, 0.5f32.sqrt());
}

#[test]
fn a_constant_block_reads_the_same_on_both() {
    let levels = Levels::of(&[-0.5; 8]);

    assert_eq!(levels.peak, 0.5);
    assert_near(levels.rms, 0.5);
}

#[test]
fn a_sine_reads_an_rms_below_its_peak() {
    let levels = Levels::of(&sine(4, 1024));

    assert_near(levels.peak, 1.0);
    assert_near(levels.rms, 0.5f32.sqrt());
}

#[test]
fn one_loud_sample_shows_in_the_peak_and_barely_in_the_rms() {
    let mut block = [0.0; 100];
    block[7] = 1.0;

    let levels = Levels::of(&block);

    assert_eq!(levels.peak, 1.0);
    assert_near(levels.rms, 0.1);
}

#[test]
fn silence_reads_zero_on_both() {
    assert_eq!(Levels::of(&[0.0; 16]), Levels::SILENT);
}

#[test]
fn a_block_with_no_samples_reads_as_silence() {
    assert_eq!(Levels::of(&[]), Levels::SILENT);
}

#[test]
fn a_meter_reads_silent_until_a_block_is_published() {
    let (_writer, reader) = level_meter();

    assert_eq!(reader.read(), Levels::SILENT);
}

#[test]
fn a_published_block_is_readable_from_the_other_end() {
    let (mut writer, reader) = level_meter();

    writer.publish(&[0.25, -0.25]);

    assert_eq!(reader.read(), Levels::of(&[0.25, -0.25]));
}

#[test]
fn a_reader_sees_the_most_recently_published_block() {
    let (mut writer, reader) = level_meter();

    writer.publish(&[1.0; 4]);
    writer.publish(&[0.1; 4]);

    assert_eq!(reader.read().peak, 0.1);
}

#[test]
fn publishing_reports_what_it_published() {
    let (mut writer, reader) = level_meter();

    let published = writer.publish(&[0.75, -0.25]);

    assert_eq!(published, reader.read());
}

#[test]
fn peak_and_rms_are_read_as_one_pair() {
    let (mut writer, reader) = level_meter();
    let stop = Arc::new(AtomicBool::new(false));
    let publishing = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                writer.publish(&[0.125; 8]);
                writer.publish(&[1.0; 8]);
            }
        })
    };

    let until = Instant::now() + SEARCH;
    while Instant::now() < until {
        let levels = reader.read();
        assert_eq!(
            levels.peak, levels.rms,
            "a constant block measures the same on both, so this pair is halves of two blocks"
        );
    }

    stop.store(true, Ordering::Relaxed);
    publishing.join().expect("the publishing thread finishes");
}

#[test]
fn publishing_does_not_allocate() {
    let (mut writer, reader) = level_meter();
    let block = [0.5; 512];

    let before = allocations();
    for _ in 0..8 {
        writer.publish(&block);
    }
    let after = allocations();

    assert_eq!(after, before);
    assert_eq!(reader.read().peak, 0.5);
}
