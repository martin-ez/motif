//! The owner of the loop, and what it makes of a block.
//!
//! The engine is the one thing holding the buffer, the transport and the
//! playhead together, so the facts worth stating are what a block comes out
//! carrying: the input it was handed, the loop mixed over that input, and
//! silence where the player asked for it.
//!
//! A player reaches it only through the queue in front of it, so every test
//! drives it the way the application thread does, and reads the playhead back
//! the way the screen does. No device is opened anywhere here — an engine that
//! needed one could not be exercised where there is no hardware.
//!
//! It runs in the callback, so the allocations are counted too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use motif::audio::{AudioPath, Command, CommandSender, Commanded, command_channel};
use motif::device::AudioProfile;
use motif::looper::{
    LoopBuffer, LoopEngine, PositionReader, Transport, WaveformReader, position_meter,
    waveform_meter,
};

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
/// Zeroed allocation is counted alongside plain allocation: the loop the engine
/// owns is a block of silence, which [`GlobalAlloc::alloc`] alone would miss.
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

fn eight_frame_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 8,
        block_size: 4,
        max_loop_seconds: 1,
    }
}

/// An engine over an eight-frame loop, with the two ends a player reaches it
/// through.
fn engine() -> (Commanded<LoopEngine>, CommandSender, PositionReader) {
    let (sender, receiver) = command_channel(8);
    let (writer, reader) = position_meter();

    (
        Commanded::new(
            receiver,
            LoopEngine::new(eight_frame_profile(), writer, waveform_meter().0),
        ),
        sender,
        reader,
    )
}

/// An engine over an eight-frame loop, with the end its shape is drawn from.
fn engine_drawing() -> (Commanded<LoopEngine>, CommandSender, WaveformReader) {
    let (sender, receiver) = command_channel(8);
    let (drawing, shape) = waveform_meter();

    (
        Commanded::new(
            receiver,
            LoopEngine::new(eight_frame_profile(), position_meter().0, drawing),
        ),
        sender,
        shape,
    )
}

/// Queue `command`, as the application thread does.
fn press(sender: &mut CommandSender, command: Command) {
    sender.send(command).expect("the queue has room for a test");
}

/// Render one block of `captured` and return what the engine played.
///
/// The block arrives silent, which is what a stream promises a path.
fn played(engine: &mut Commanded<LoopEngine>, captured: &[f32]) -> Vec<f32> {
    let mut playing = vec![0.0; captured.len()];
    engine.render(captured, &mut playing);

    playing
}

/// Render one block of silence `frames` long and return what the engine played.
fn heard(engine: &mut Commanded<LoopEngine>, frames: usize) -> Vec<f32> {
    played(engine, &vec![0.0; frames])
}

#[test]
fn the_allocation_counter_counts_an_allocation() {
    let before = allocations();
    black_box(Vec::<f32>::with_capacity(4));
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
#[should_panic(expected = "block")]
fn a_profile_with_no_block_is_refused_at_setup() {
    let (writer, _position) = position_meter();

    LoopEngine::new(
        AudioProfile {
            block_size: 0,
            ..eight_frame_profile()
        },
        writer,
        waveform_meter().0,
    );
}

#[test]
fn an_engine_answers_every_command_there_is() {
    let (writer, _position) = position_meter();
    let mut engine = LoopEngine::new(eight_frame_profile(), writer, waveform_meter().0);

    assert!(engine.apply(Command::SetTransport(Transport::Recording)));
    assert!(engine.apply(Command::SetGain(0.5)));
    assert!(engine.apply(Command::SetMuted(true)));
    assert!(engine.apply(Command::Undo));
    assert!(engine.apply(Command::Clear));
}

#[test]
fn an_idle_engine_plays_the_input_it_was_handed() {
    let (mut engine, _sender, _position) = engine();

    assert_eq!(played(&mut engine, &[0.25, 0.5]), [0.25, 0.5]);
}

#[test]
fn a_take_is_heard_once_the_transport_plays_it() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.25, 0.5]);
}

#[test]
fn a_playing_loop_is_mixed_over_the_live_input() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(played(&mut engine, &[0.125, 0.125]), [0.375, 0.625]);
}

#[test]
fn the_loop_repeats_across_a_block_boundary() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5, 0.75]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 5), [0.25, 0.5, 0.75, 0.25, 0.5]);
}

#[test]
fn a_take_longer_than_one_block_is_recorded_whole() {
    let (mut engine, mut sender, position) = engine();
    let take = [0.25, 0.5, 0.75, 0.125, 0.375, 0.625];

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &take);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, take.len()), take);
    assert_eq!(position.read().recorded(), 6);
}

#[test]
fn an_overdub_lands_on_top_of_the_take() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.125]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.375, 0.625]);
}

#[test]
fn an_overdub_is_heard_over_the_loop_as_it_is_recorded() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));

    assert_eq!(played(&mut engine, &[0.125, 0.125]), [0.375, 0.625]);
}

#[test]
fn a_take_takes_an_overdub_for_every_layer_over_it() {
    let (mut engine, mut sender, _position) = engine();
    let layered = 0.125;

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 1);
    for _ in 1..LoopBuffer::LAYERS {
        press(&mut sender, Command::SetTransport(Transport::Overdubbing));
        played(&mut engine, &[layered]);
        press(&mut sender, Command::SetTransport(Transport::Playing));
        heard(&mut engine, 1);
    }

    let over_the_take = layered * (LoopBuffer::LAYERS - 1) as f32;
    assert_eq!(heard(&mut engine, 1), [0.25 + over_the_take]);
}

#[test]
fn an_overdub_opened_mid_loop_lands_at_the_top_of_the_loop() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5, 0.75, 1.0]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 4), [0.375, 0.5, 0.75, 1.0]);
}

#[test]
fn an_overdub_past_the_layer_stack_leaves_the_loop_alone() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    for _ in 1..LoopBuffer::LAYERS {
        press(&mut sender, Command::SetTransport(Transport::Playing));
        heard(&mut engine, 1);
        press(&mut sender, Command::SetTransport(Transport::Overdubbing));
        heard(&mut engine, 1);
    }
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 1);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[1.0]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.25, 0.5]);
}

#[test]
fn undo_takes_nothing_further_into_the_loop_until_the_transport_changes() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.125]);
    press(&mut sender, Command::Undo);
    played(&mut engine, &[1.0, 1.0]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.25, 0.5]);
}

#[test]
fn undo_with_no_overdub_to_take_off_leaves_the_take_open() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25]);
    press(&mut sender, Command::Undo);
    played(&mut engine, &[0.5]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.25, 0.5]);
}

#[test]
fn clearing_a_loop_leaves_the_take_open_for_the_next_one() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::Clear);
    played(&mut engine, &[0.75]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 1), [0.75]);
}

#[test]
fn stopping_a_stopped_loop_keeps_its_playhead() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5, 0.75]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    heard(&mut engine, 2);

    assert_eq!(position.read().playhead(), 2);
}

#[test]
fn undo_takes_the_overdub_back_off() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.125]);
    press(&mut sender, Command::Undo);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.25, 0.5]);
}

#[test]
fn clear_empties_the_loop() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::Clear);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.0, 0.0]);
    assert_eq!(position.read().recorded(), 0);
}

#[test]
fn the_playhead_follows_the_loop_across_its_boundary() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5, 0.75]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);
    heard(&mut engine, 2);

    assert_eq!(position.read().playhead(), 1);
    assert_eq!(position.read().recorded(), 3);
}

#[test]
fn a_take_publishes_the_end_of_what_it_has_recorded() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);

    assert_eq!(position.read().playhead(), 2);
    assert_eq!(position.read().recorded(), 2);
}

#[test]
fn playing_after_a_stop_restarts_the_loop() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5, 0.75]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 1), [0.25]);
}

#[test]
fn a_stopped_engine_plays_the_input_and_none_of_the_loop() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));

    assert_eq!(played(&mut engine, &[0.125, 0.125]), [0.125, 0.125]);
}

#[test]
fn a_muted_engine_plays_silence() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetMuted(true));

    assert_eq!(played(&mut engine, &[0.25, 0.5]), [0.0, 0.0]);
}

#[test]
fn muting_the_output_does_not_stop_the_take() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetMuted(true));
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetMuted(false));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 2), [0.25, 0.5]);
}

#[test]
fn gain_scales_the_input_that_reaches_the_loop() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetGain(0.5));
    press(&mut sender, Command::SetTransport(Transport::Recording));

    assert_eq!(played(&mut engine, &[0.5, 1.0]), [0.25, 0.5]);
}

#[test]
fn a_pair_of_commands_both_land_on_the_next_block() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    press(&mut sender, Command::SetMuted(true));

    assert_eq!(heard(&mut engine, 2), [0.0, 0.0]);
}

#[test]
fn a_block_shorter_than_the_input_records_only_what_it_played() {
    let (mut engine, mut sender, position) = engine();
    let mut playing = [0.0; 2];
    press(&mut sender, Command::SetTransport(Transport::Recording));

    engine.render(&[0.25, 0.5, 0.75], &mut playing);

    assert_eq!(playing, [0.25, 0.5]);
    assert_eq!(position.read().recorded(), 2);
}

#[test]
fn recording_a_block_does_not_allocate() {
    let (mut engine, mut sender, _position) = engine();
    let captured = vec![0.25; 4];
    let mut playing = vec![0.0; 4];
    press(&mut sender, Command::SetTransport(Transport::Recording));

    let before = allocations();
    engine.render(&captured, &mut playing);
    let after = allocations();

    assert_eq!(after, before, "recording a block allocated");
}

#[test]
fn playing_the_loop_over_the_input_does_not_allocate() {
    let (mut engine, mut sender, _position) = engine();
    let captured = vec![0.25; 4];
    let mut playing = vec![0.0; 4];
    press(&mut sender, Command::SetTransport(Transport::Recording));
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));

    let before = allocations();
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    engine.render(&captured, &mut playing);
    let after = allocations();

    assert_eq!(after, before, "playing a block allocated");
}

#[test]
fn a_block_longer_than_one_the_profile_states_does_not_allocate() {
    let (mut engine, mut sender, _position) = engine();
    let captured = vec![0.25; 6];
    let mut playing = vec![0.0; 6];
    press(&mut sender, Command::SetTransport(Transport::Recording));

    let before = allocations();
    engine.render(&captured, &mut playing);
    let after = allocations();

    assert_eq!(after, before, "a block over the block size allocated");
}

#[test]
fn undoing_and_clearing_do_not_allocate() {
    let (mut engine, mut sender, _position) = engine();
    let captured = vec![0.25; 4];
    let mut playing = vec![0.0; 4];
    press(&mut sender, Command::SetTransport(Transport::Recording));
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    engine.render(&captured, &mut playing);

    let before = allocations();
    press(&mut sender, Command::Undo);
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::Clear);
    engine.render(&captured, &mut playing);
    let after = allocations();

    assert_eq!(after, before, "undo or clear allocated");
}

#[test]
fn the_engine_publishes_the_shape_of_what_it_recorded() {
    let (mut engine, mut sender, shape) = engine_drawing();
    press(&mut sender, Command::SetTransport(Transport::Recording));

    played(&mut engine, &[0.25, -0.5]);

    assert_eq!(shape.read().buckets().len(), 2);
    assert_eq!(shape.read().buckets()[0].peak, 0.25);
}

#[test]
fn the_published_shape_is_of_the_input_the_gain_let_through() {
    let (mut engine, mut sender, shape) = engine_drawing();
    press(&mut sender, Command::SetGain(0.5));
    press(&mut sender, Command::SetTransport(Transport::Recording));

    played(&mut engine, &[1.0]);

    assert_eq!(shape.read().buckets()[0].peak, 0.5);
}

#[test]
fn a_cleared_loop_publishes_no_shape() {
    let (mut engine, mut sender, shape) = engine_drawing();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, -0.5]);

    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::Clear);
    played(&mut engine, &[0.75, 0.75]);

    assert!(shape.read().buckets().is_empty());
}

#[test]
fn undoing_heals_the_published_shape_within_a_lap() {
    let (mut engine, mut sender, shape) = engine_drawing();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    heard(&mut engine, 8);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.5; 8]);

    press(&mut sender, Command::Undo);
    heard(&mut engine, 8);

    assert!(
        shape
            .read()
            .buckets()
            .iter()
            .all(|bucket| bucket.peak == 0.0),
        "the undone layer is still in the published shape"
    );
}

#[test]
fn the_shape_of_an_undone_layer_survives_until_the_sweep_reaches_it() {
    let (mut engine, mut sender, shape) = engine_drawing();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    heard(&mut engine, 8);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.5; 8]);

    press(&mut sender, Command::Undo);
    heard(&mut engine, 4);

    let shape = shape.read();
    assert_eq!(shape.buckets()[0].peak, 0.0);
    assert_eq!(shape.buckets()[7].peak, 0.5);
}

#[test]
fn healing_the_shape_after_an_undo_does_not_allocate() {
    let (mut engine, mut sender, _shape) = engine_drawing();
    let captured = vec![0.25; 4];
    let mut playing = vec![0.0; 4];
    press(&mut sender, Command::SetTransport(Transport::Recording));
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::Undo);

    let before = allocations();
    for _ in 0..LoopBuffer::LAYERS {
        engine.render(&captured, &mut playing);
    }
    let after = allocations();

    assert_eq!(after, before, "healing the shape allocated");
}
