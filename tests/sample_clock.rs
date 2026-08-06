//! The frame count the callback keeps, and the thread that stamps a tap with
//! it.
//!
//! The facts worth stating are that the count starts at nothing, that it
//! accumulates block by block, that it carries the rate those frames are
//! counted at, that reading it takes nothing away, that a reader on another
//! thread never sees it go backwards, and that advancing it allocates nothing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::{
    AudioBackend, AudioPath, Command, Counting, DeviceSelection, NullBackend, Passthrough,
    StreamConfig, StreamRequest, sample_clock,
};
use motif::device::{AudioProfile, Button, DeviceProfile};
use motif::looper::LooperPage;
use motif::seq::TapTempo;
use motif::ui::{ControlEvent, Page};

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

/// Half a second of frames at [`SAMPLE_RATE`], which is 120 BPM there and
/// 130.6 at the rate the target profile asks for.
const HALF_A_ROUNDED_SECOND: usize = SAMPLE_RATE as usize / 2;

/// How long the two-thread test spends looking for a count that went backwards.
const SEARCH: Duration = Duration::from_millis(50);

const AUDIO: AudioProfile = DeviceProfile::TARGET.audio;

fn granted() -> StreamConfig {
    StreamConfig {
        sample_rate: AUDIO.sample_rate,
        block_size: AUDIO.block_size,
        input_channels: 2,
        output_channels: 2,
    }
}

/// What a device that rounds grants against [`request`]: the same stream at a
/// rate the request did not ask for.
fn rounded() -> StreamConfig {
    StreamConfig {
        sample_rate: SAMPLE_RATE,
        ..granted()
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: AUDIO.sample_rate,
        block_size: AUDIO.block_size,
    }
}

fn selection() -> DeviceSelection {
    NullBackend::rounding(granted())
        .defaults(AUDIO.sample_rate)
        .expect("the null backend has a device in each direction")
}

fn shifted(button: Button) -> ControlEvent {
    ControlEvent::Pressed {
        button,
        shifted: true,
    }
}

/// A path that keeps the configuration it was prepared with, so that a test can
/// read it back through the path wrapped around it.
#[derive(Clone, Default)]
struct Prepared(Arc<Mutex<Option<StreamConfig>>>);

impl Prepared {
    fn config(&self) -> Option<StreamConfig> {
        *self.0.lock().expect("no test holds this across a panic")
    }
}

impl AudioPath for Prepared {
    fn prepare(&mut self, config: StreamConfig) {
        *self.0.lock().expect("no test holds this across a panic") = Some(config);
    }

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// A path that takes whatever command it is offered, so that a test can read
/// back what reached it through the path wrapped around it.
#[derive(Clone, Default)]
struct Answering(Arc<Mutex<Option<Command>>>);

impl Answering {
    fn taken(&self) -> Option<Command> {
        *self.0.lock().expect("no test holds this across a panic")
    }
}

impl AudioPath for Answering {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, command: Command) -> bool {
        *self.0.lock().expect("no test holds this across a panic") = Some(command);
        true
    }
}

#[test]
fn a_rendered_block_moves_the_clock_on_by_its_frames() {
    let (frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut path = Counting::new(frames, Passthrough::new());

    path.render(&[0.0; BLOCK], &mut [0.0; BLOCK]);

    assert_eq!(elapsed.read(), BLOCK as u64);
}

#[test]
fn rendered_blocks_accumulate_on_the_clock() {
    let (frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut path = Counting::new(frames, Passthrough::new());

    for _ in 0..4 {
        path.render(&[0.0; BLOCK], &mut [0.0; BLOCK]);
    }

    assert_eq!(elapsed.read(), 4 * BLOCK as u64);
}

#[test]
fn a_counting_path_plays_what_the_path_it_wraps_plays() {
    let mut path = Counting::new(sample_clock(SAMPLE_RATE).0, Passthrough::new());
    let mut played = [0.0; 3];

    path.render(&[0.25, 0.5, 0.75], &mut played);

    assert_eq!(played, [0.25, 0.5, 0.75]);
}

#[test]
fn a_counting_path_prepares_the_path_it_wraps() {
    let wrapped = Prepared::default();
    let mut path = Counting::new(sample_clock(SAMPLE_RATE).0, wrapped.clone());

    path.prepare(granted());

    assert_eq!(wrapped.config(), Some(granted()));
}

#[test]
fn a_counting_path_offers_a_command_to_the_path_it_wraps() {
    let wrapped = Answering::default();
    let mut path = Counting::new(sample_clock(SAMPLE_RATE).0, wrapped.clone());

    path.apply(Command::Clear);

    assert_eq!(wrapped.taken(), Some(Command::Clear));
}

#[test]
fn a_counting_path_answers_a_command_the_path_it_wraps_took() {
    let mut path = Counting::new(sample_clock(SAMPLE_RATE).0, Answering::default());

    assert!(path.apply(Command::Clear));
}

#[test]
fn a_counting_path_answers_nothing_the_path_it_wraps_refused() {
    let mut path = Counting::new(sample_clock(SAMPLE_RATE).0, Prepared::default());

    assert!(!path.apply(Command::Clear));
}

#[test]
fn a_block_counts_the_frames_both_its_slices_carried() {
    let (frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut path = Counting::new(frames, Passthrough::new());

    path.render(&[0.0; BLOCK], &mut [0.0; BLOCK / 2]);

    assert_eq!(elapsed.read(), BLOCK as u64 / 2);
}

#[test]
fn rendering_through_a_counting_path_does_not_allocate() {
    let (frames, elapsed) = sample_clock(SAMPLE_RATE);
    let mut path = Counting::new(frames, Passthrough::new());
    let mut played = [0.0; BLOCK];

    let before = allocations();
    for _ in 0..8 {
        path.render(&[0.0; BLOCK], &mut played);
    }
    let after = allocations();

    assert_eq!(after, before, "rendering through the clock allocated");
    assert_eq!(elapsed.read(), 8 * BLOCK as u64);
}

#[test]
fn a_stream_advances_the_clock_its_path_was_given() {
    let (frames, elapsed) = sample_clock(AUDIO.sample_rate);
    let mut stream = NullBackend::rounding(granted())
        .open(
            &selection(),
            request(),
            Counting::new(frames, Passthrough::new()),
        )
        .expect("null backend opens");

    stream.block(&[0.0; BLOCK], &mut [0.0; BLOCK]);

    assert_eq!(elapsed.read(), BLOCK as u64);
}

#[test]
fn a_tap_is_stamped_with_the_frames_the_stream_has_played() {
    let (frames, elapsed) = sample_clock(AUDIO.sample_rate);
    let (mut page, engine, _takes) = LooperPage::driving(AUDIO, elapsed);
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), Counting::new(frames, engine))
        .expect("null backend opens");
    stream.block(&[0.0; BLOCK], &mut [0.0; BLOCK]);

    page.control(shifted(Button::Play));

    assert_eq!(page.grid().beats(), [BLOCK as u64]);
}

#[test]
fn a_counting_path_states_the_granted_rate_to_the_clock() {
    let (frames, elapsed) = sample_clock(AUDIO.sample_rate);
    let mut path = Counting::new(frames, Passthrough::new());

    path.prepare(rounded());

    assert_eq!(elapsed.sample_rate(), SAMPLE_RATE);
}

#[test]
fn a_clock_reports_the_rate_the_stream_granted() {
    let (frames, elapsed) = sample_clock(AUDIO.sample_rate);

    let _stream = NullBackend::rounding(rounded())
        .open(
            &selection(),
            request(),
            Counting::new(frames, Passthrough::new()),
        )
        .expect("null backend opens");

    assert_eq!(elapsed.sample_rate(), SAMPLE_RATE);
}

#[test]
fn a_tap_is_timed_at_the_rate_the_device_granted() {
    let (frames, elapsed) = sample_clock(AUDIO.sample_rate);
    let (mut page, engine, _takes) = LooperPage::driving(AUDIO, elapsed);
    let mut stream = NullBackend::rounding(rounded())
        .open(&selection(), request(), Counting::new(frames, engine))
        .expect("null backend opens");

    for _ in 0..TapTempo::TAPS_TO_A_TEMPO {
        page.control(shifted(Button::Play));
        stream.block(
            &[0.0; HALF_A_ROUNDED_SECOND],
            &mut [0.0; HALF_A_ROUNDED_SECOND],
        );
    }

    assert_eq!(page.grid().beats_per_minute(), Some(120.0));
}

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    std::hint::black_box(Vec::<u64>::with_capacity(4));
    let after = allocations();

    assert!(after > before, "the counter is not wired to the allocator");
}

#[test]
fn the_allocation_counter_counts_a_zeroed_allocation() {
    let before = allocations();
    std::hint::black_box(vec![0.0_f32; 4]);
    let after = allocations();

    assert!(
        after > before,
        "the counter is not wired to zeroed allocation"
    );
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
