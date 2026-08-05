//! The single-producer single-consumer ring that carries samples across the
//! audio boundary.
//!
//! The callback holds one end and the application thread the other, so the
//! facts worth stating are that samples survive the crossing intact, that a
//! full or an empty ring is reported rather than waited on, and that neither
//! end allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::{black_box, spin_loop};
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::sample_ring;

/// How long a concurrent test goes without moving a single sample before it
/// decides the ring has stalled. It bounds a run of fruitless attempts rather
/// than the test as a whole, so a slow machine only makes the test slow, while
/// a ring that has stopped moving samples fails rather than hangs.
const PATIENCE: Duration = Duration::from_secs(5);

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

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    black_box(Vec::<f32>::with_capacity(4));
    let after = allocations();

    assert!(after > before, "the counter is not wired to the allocator");
}

#[test]
fn samples_are_read_back_in_the_order_they_were_written() {
    let (mut producer, mut consumer) = sample_ring(8);
    let mut taken = [0.0; 3];

    producer.write(&[1.0, 2.0, 3.0]);
    consumer.read(&mut taken);

    assert_eq!(taken, [1.0, 2.0, 3.0]);
}

#[test]
fn a_write_reports_how_many_samples_it_took() {
    let (mut producer, _consumer) = sample_ring(8);

    assert_eq!(producer.write(&[1.0, 2.0, 3.0]), 3);
}

#[test]
fn a_write_that_does_not_fit_takes_what_it_can_and_says_so() {
    let (mut producer, _consumer) = sample_ring(4);

    assert_eq!(producer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 4);
}

#[test]
fn a_write_to_a_full_ring_takes_nothing() {
    let (mut producer, _consumer) = sample_ring(4);
    producer.write(&[1.0, 2.0, 3.0, 4.0]);

    assert_eq!(producer.write(&[5.0]), 0);
}

#[test]
fn a_read_of_an_empty_ring_takes_nothing() {
    let (_producer, mut consumer) = sample_ring(8);
    let mut taken = [0.0; 4];

    assert_eq!(consumer.read(&mut taken), 0);
}

#[test]
fn a_read_of_a_partly_filled_ring_reports_how_much_it_got() {
    let (mut producer, mut consumer) = sample_ring(8);
    let mut taken = [0.0; 4];
    producer.write(&[1.0, 2.0]);

    assert_eq!(consumer.read(&mut taken), 2);
}

#[test]
fn a_read_leaves_the_rest_of_the_output_untouched() {
    let (mut producer, mut consumer) = sample_ring(8);
    let mut taken = [9.0; 4];
    producer.write(&[1.0]);

    consumer.read(&mut taken);

    assert_eq!(taken, [1.0, 9.0, 9.0, 9.0]);
}

#[test]
fn a_write_that_does_not_fit_leaves_the_samples_it_took_intact() {
    let (mut producer, mut consumer) = sample_ring(4);
    let mut taken = [0.0; 4];

    producer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    consumer.read(&mut taken);

    assert_eq!(taken, [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn a_read_leaves_the_samples_it_did_not_take() {
    let (mut producer, mut consumer) = sample_ring(8);
    let mut first = [0.0; 2];
    let mut second = [0.0; 2];
    producer.write(&[1.0, 2.0, 3.0, 4.0]);

    consumer.read(&mut first);
    consumer.read(&mut second);

    assert_eq!((first, second), ([1.0, 2.0], [3.0, 4.0]));
}

#[test]
fn reading_frees_the_slots_it_took() {
    let (mut producer, mut consumer) = sample_ring(4);
    let mut taken = [0.0; 2];
    producer.write(&[1.0, 2.0, 3.0, 4.0]);

    consumer.read(&mut taken);

    assert_eq!(producer.write(&[5.0, 6.0]), 2);
}

#[test]
fn samples_survive_wrapping_around_the_end_of_the_ring() {
    let (mut producer, mut consumer) = sample_ring(4);
    let mut taken = [0.0; 3];
    producer.write(&[1.0, 2.0, 3.0]);
    consumer.read(&mut taken);

    producer.write(&[4.0, 5.0, 6.0]);
    consumer.read(&mut taken);

    assert_eq!(taken, [4.0, 5.0, 6.0]);
}

#[test]
fn samples_keep_their_order_when_the_wrap_falls_unevenly() {
    let (mut producer, mut consumer) = sample_ring(8);
    let mut discarded = [0.0; 2];
    let mut taken = [0.0; 7];
    producer.write(&[0.0, 0.0]);
    consumer.read(&mut discarded);

    producer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    consumer.read(&mut taken);

    assert_eq!(taken, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
}

#[test]
fn a_capacity_that_is_not_a_power_of_two_wraps_cleanly() {
    let (mut producer, mut consumer) = sample_ring(3);
    let mut taken = [0.0; 2];
    let mut received = Vec::new();

    for pair in 0..64 {
        let first = (pair * 2) as f32;
        producer.write(&[first, first + 1.0]);
        let count = consumer.read(&mut taken);
        received.extend_from_slice(&taken[..count]);
    }

    let expected: Vec<f32> = (0..128).map(|index| index as f32).collect();
    assert_eq!(received, expected);
}

#[test]
fn a_ring_reports_the_capacity_it_was_built_with() {
    let (producer, consumer) = sample_ring(64);

    assert_eq!((producer.capacity(), consumer.capacity()), (64, 64));
}

#[test]
fn vacant_slots_fall_as_the_ring_fills() {
    let (mut producer, _consumer) = sample_ring(8);
    producer.write(&[1.0, 2.0, 3.0]);

    assert_eq!(producer.vacant(), 5);
}

#[test]
fn available_samples_rise_as_the_ring_fills() {
    let (mut producer, consumer) = sample_ring(8);
    producer.write(&[1.0, 2.0, 3.0]);

    assert_eq!(consumer.available(), 3);
}

#[test]
#[should_panic(expected = "capacity")]
fn a_ring_with_no_capacity_is_refused_at_setup() {
    sample_ring(0);
}

#[test]
fn a_ring_of_one_sample_still_carries_them_one_at_a_time() {
    let (mut producer, mut consumer) = sample_ring(1);
    let mut taken = [0.0; 1];

    producer.write(&[1.0]);
    consumer.read(&mut taken);
    producer.write(&[2.0]);
    consumer.read(&mut taken);

    assert_eq!(taken, [2.0]);
}

#[test]
fn every_sample_survives_a_concurrent_round_trip() {
    const SAMPLES: usize = 100_000;
    const BLOCK: usize = 64;

    let (mut producer, mut consumer) = sample_ring(256);
    let sent: Vec<f32> = (0..SAMPLES).map(|index| index as f32).collect();
    let mut received = Vec::with_capacity(SAMPLES);

    thread::scope(|scope| {
        scope.spawn(move || {
            let mut deadline = Instant::now() + PATIENCE;
            let mut offset = 0;
            while offset < SAMPLES {
                let block = &sent[offset..(offset + BLOCK).min(SAMPLES)];
                let written = producer.write(block);
                if written == 0 {
                    assert!(Instant::now() < deadline, "the ring never made room");
                    spin_loop();
                } else {
                    deadline = Instant::now() + PATIENCE;
                }
                offset += written;
            }
        });

        let mut deadline = Instant::now() + PATIENCE;
        let mut block = [0.0; BLOCK];
        while received.len() < SAMPLES {
            let read = consumer.read(&mut block);
            if read == 0 {
                assert!(Instant::now() < deadline, "the ring never delivered");
                spin_loop();
            } else {
                deadline = Instant::now() + PATIENCE;
            }
            received.extend_from_slice(&block[..read]);
        }
    });

    let expected: Vec<f32> = (0..SAMPLES).map(|index| index as f32).collect();
    assert_eq!(received, expected);
}

#[test]
fn neither_end_allocates() {
    let (mut producer, mut consumer) = sample_ring(256);
    let block = [0.25; 64];
    let mut taken = [0.0; 64];

    let before = allocations();
    for _ in 0..8 {
        producer.write(&block);
        consumer.read(&mut taken);
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn neither_end_allocates_when_the_ring_is_full_or_empty() {
    let (mut producer, mut consumer) = sample_ring(4);
    let block = [0.25; 8];
    let mut taken = [0.0; 8];

    let before = allocations();
    producer.write(&block);
    producer.write(&block);
    consumer.read(&mut taken);
    consumer.read(&mut taken);
    let after = allocations();

    assert_eq!(after, before);
}
