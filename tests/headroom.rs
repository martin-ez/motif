//! How much of a block period the audio callback spent working, measured in the
//! callback and read from another thread.
//!
//! The facts worth stating are the ratio itself — work against the period of the
//! block that was actually handed over — that a spike survives long enough to be
//! seen, that the pair always reads as one block's, that reading is a look
//! rather than a take, and that none of it allocates.
//!
//! The clock is tested here too, though `Instant::now` is not this crate's code:
//! it is what the callback reads, and a clock that allocated would break the
//! real-time invariant from a place no other test looks.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::{
    AudioBackend, DuplexStream, Headroom, NullBackend, StreamConfig, StreamRequest, headroom_meter,
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

const SAMPLE_RATE: u32 = 48_000;

/// A block of 48 frames at 48 kHz, whose period is exactly one millisecond.
const BLOCK: usize = 48;

const PERIOD: Duration = Duration::from_millis(1);

/// Frames enough to close the recent window twice over, so that whatever the
/// window holds has been replaced rather than merely added to.
const PAST_THE_WINDOW: usize = SAMPLE_RATE as usize;

const GRANTED: StreamConfig = StreamConfig {
    sample_rate: SAMPLE_RATE,
    block_size: BLOCK as u32,
    input_channels: 1,
    output_channels: 2,
};

fn quiet_blocks(writer: &mut motif::audio::HeadroomWriter, frames: usize) {
    let mut covered = 0;
    while covered < frames {
        writer.measured(Duration::ZERO, BLOCK);
        covered += BLOCK;
    }
}

#[test]
fn a_meter_starts_idle() {
    let (_writer, reader) = headroom_meter(SAMPLE_RATE);

    assert_eq!(reader.read(), Headroom::IDLE);
}

#[test]
fn a_block_that_used_half_its_period_reads_half() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD / 2, BLOCK);

    assert_eq!(reader.read().load, 0.5);
}

#[test]
fn a_block_that_used_all_of_its_period_reads_one() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD, BLOCK);

    assert_eq!(reader.read().load, 1.0);
}

#[test]
fn a_block_that_overran_its_period_reads_above_one() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD * 2, BLOCK);

    assert_eq!(reader.read().load, 2.0);
}

#[test]
fn the_same_work_over_more_frames_is_less_of_the_period() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD, BLOCK);
    let over_one_block = reader.read().load;
    writer.measured(PERIOD, BLOCK * 2);

    assert_eq!(over_one_block, 1.0);
    assert_eq!(reader.read().load, 0.5);
}

#[test]
fn the_load_read_is_the_last_block_measured() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD, BLOCK);
    writer.measured(PERIOD / 4, BLOCK);

    assert_eq!(reader.read().load, 0.25);
}

#[test]
fn measuring_reports_what_it_published() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    let measured = writer.measured(PERIOD / 2, BLOCK);

    assert_eq!(measured, reader.read());
}

#[test]
fn the_peak_holds_a_spike_a_later_block_did_not_repeat() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD, BLOCK);
    writer.measured(Duration::ZERO, BLOCK);

    assert_eq!(reader.read().peak, 1.0);
}

#[test]
fn the_peak_falls_back_once_the_spike_has_left_the_window() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD, BLOCK);
    quiet_blocks(&mut writer, PAST_THE_WINDOW);

    assert_eq!(reader.read().peak, 0.0);
}

#[test]
fn a_spike_survives_a_reader_looking_a_frame_later() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);
    let a_frame = (SAMPLE_RATE / 30) as usize;

    writer.measured(PERIOD, BLOCK);
    quiet_blocks(&mut writer, a_frame);

    assert_eq!(reader.read().peak, 1.0);
}

#[test]
fn the_load_never_reads_above_the_peak_it_arrives_with() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    for block in 0..PAST_THE_WINDOW / BLOCK {
        writer.measured(PERIOD * (block as u32 % 7), BLOCK);
        let read = reader.read();

        assert!(read.load <= read.peak);
    }
}

#[test]
fn reading_does_not_reset_what_was_measured() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD / 2, BLOCK);

    assert_eq!(reader.read(), reader.read());
}

#[test]
fn spare_is_what_the_worst_recent_block_left_unused() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD / 4, BLOCK);

    assert_eq!(reader.read().spare(), 0.75);
}

#[test]
fn spare_is_negative_where_a_block_overran() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD * 2, BLOCK);

    assert_eq!(reader.read().spare(), -1.0);
}

#[test]
fn an_idle_callback_has_all_of_its_period_spare() {
    assert_eq!(Headroom::IDLE.spare(), 1.0);
}

#[test]
fn the_worse_of_two_callbacks_takes_each_number_from_the_larger() {
    let busy_now = Headroom {
        load: 0.6,
        peak: 0.7,
    };
    let busy_before = Headroom {
        load: 0.1,
        peak: 0.9,
    };

    assert_eq!(
        busy_now.worse_of(busy_before),
        Headroom {
            load: 0.6,
            peak: 0.9
        }
    );
}

#[test]
fn the_worse_of_two_callbacks_does_not_depend_on_their_order() {
    let one = Headroom {
        load: 0.6,
        peak: 0.7,
    };
    let other = Headroom {
        load: 0.1,
        peak: 0.9,
    };

    assert_eq!(one.worse_of(other), other.worse_of(one));
}

#[test]
fn a_block_of_no_frames_is_not_measured() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    writer.measured(PERIOD / 2, BLOCK);
    writer.measured(PERIOD, 0);

    assert_eq!(reader.read().load, 0.5);
}

#[test]
fn a_meter_with_no_sample_rate_measures_nothing() {
    let (mut writer, reader) = headroom_meter(0);

    writer.measured(PERIOD, BLOCK);

    assert_eq!(reader.read(), Headroom::IDLE);
}

#[test]
fn a_measurement_crosses_from_the_thread_that_made_it() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    let measuring = thread::spawn(move || {
        writer.measured(PERIOD / 2, BLOCK);
    });
    measuring.join().expect("the measuring thread finishes");

    assert_eq!(reader.read().load, 0.5);
}

#[test]
fn measuring_does_not_allocate() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);

    let before = allocations();
    for _ in 0..PAST_THE_WINDOW / BLOCK {
        writer.measured(PERIOD / 2, BLOCK);
    }
    let after = allocations();

    assert_eq!(after, before);
    assert_eq!(reader.read().load, 0.5);
}

#[test]
fn reading_does_not_allocate() {
    let (mut writer, reader) = headroom_meter(SAMPLE_RATE);
    writer.measured(PERIOD / 2, BLOCK);

    let before = allocations();
    for _ in 0..128 {
        reader.read();
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn reading_the_clock_does_not_allocate() {
    let started = Instant::now();

    let before = allocations();
    for _ in 0..128 {
        let _ = started.elapsed();
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn a_stream_that_moves_no_samples_reports_an_idle_callback() {
    let stream = NullBackend::rounding(GRANTED)
        .open(StreamRequest {
            sample_rate: GRANTED.sample_rate,
            block_size: GRANTED.block_size,
        })
        .expect("a rounding backend opens whatever it is asked for");

    assert_eq!(stream.headroom(), Headroom::IDLE);
}
