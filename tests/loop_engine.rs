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

use motif::audio::{
    AudioPath, Command, CommandSender, Commanded, Gain, StreamConfig, command_channel,
};
use motif::device::AudioProfile;
use motif::looper::{
    LoopBuffer, LoopEngine, PositionReader, TakeReader, TakeWriter, Transport, WaveformReader,
    position_meter, take_handoff, waveform_meter,
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

/// More blocks than any crossing in these tests can need, so a take that never
/// crosses fails rather than hanging.
///
/// Pinned here rather than read off `TakeWriter::CROSSING_BLOCKS`, which a
/// mutant is free to make enormous.
const BLOCKS_ALLOWED: usize = 16;

/// More blocks than a crossing at the smallest block these tests grant can
/// need, so a take that never crosses fails rather than hanging.
const CROSSINGS_ALLOWED: usize = 512;

fn eight_frame_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 8,
        block_size: 4,
        max_loop_seconds: 1,
    }
}

/// A profile whose longest loop is twice the blocks a take gets to cross in, so
/// a full block's share is two frames and half a block's is one.
///
/// A share of one frame is the floor, so a loop any shorter than this crosses
/// in the same blocks whatever it was handed.
/// A profile whose ten milliseconds of ramp is forty frames, where the
/// eight-frame profile's is under one and a level change arrives in a single
/// frame.
fn ramping_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 4_000,
        block_size: 16,
        max_loop_seconds: 1,
    }
}

/// Frames to hold a level change over: longer than `ramping_profile`'s ramp, so
/// the change has arrived by the last of them, and several blocks' worth, so a
/// ramp that restarted at a block boundary would not reach it.
const RAMPING_FRAMES: usize = 64;

/// The largest step `ramping_profile`'s ramp may take between two frames, with
/// room for what a linear walk accumulates.
///
/// Pinned rather than derived from `Gain::RAMP` and the profile's rate, which a
/// mutant is free to move. The change the level makes is twenty-five times this
/// if nothing spreads it.
const LARGEST_STEP: f32 = 0.03;

/// The stream a device granting `ramping_profile` would report.
fn ramping_config() -> StreamConfig {
    let profile = ramping_profile();

    StreamConfig {
        sample_rate: profile.sample_rate,
        block_size: profile.block_size,
        input_channels: 1,
        output_channels: 1,
    }
}

fn scaling_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: 128,
        block_size: 8,
        max_loop_seconds: 1,
    }
}

/// The publishing end of a handoff whose takes no test reads.
fn crossing() -> TakeWriter {
    take_handoff(eight_frame_profile()).0
}

/// An engine over an eight-frame loop, with the two ends a player reaches it
/// through.
fn engine() -> (Commanded<LoopEngine>, CommandSender, PositionReader) {
    engine_at(eight_frame_profile())
}

/// An engine over `profile`'s longest loop, with the two ends a player reaches
/// it through.
fn engine_at(profile: AudioProfile) -> (Commanded<LoopEngine>, CommandSender, PositionReader) {
    let (sender, receiver) = command_channel(8);
    let (writer, reader) = position_meter();

    (
        Commanded::new(
            receiver,
            LoopEngine::new(profile, writer, waveform_meter().0, take_handoff(profile).0),
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
            LoopEngine::new(
                eight_frame_profile(),
                position_meter().0,
                drawing,
                crossing(),
            ),
        ),
        sender,
        shape,
    )
}

/// An engine over `profile`'s longest loop, with the end its takes are read
/// from.
fn engine_over(profile: AudioProfile) -> (Commanded<LoopEngine>, CommandSender, TakeReader) {
    let (sender, receiver) = command_channel(8);
    let (crossing, takes) = take_handoff(profile);

    (
        Commanded::new(
            receiver,
            LoopEngine::new(profile, position_meter().0, waveform_meter().0, crossing),
        ),
        sender,
        takes,
    )
}

/// An engine over an eight-frame loop, with the end its takes are read from.
fn engine_handing_takes() -> (Commanded<LoopEngine>, CommandSender, TakeReader) {
    engine_over(eight_frame_profile())
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

/// Render the frame a queued level change is walked over.
///
/// Ten milliseconds of ramp is under a frame at `eight_frame_profile`'s rate,
/// so one frame is the whole of it. A level read off the block that queued it
/// would be the ramp rather than the level.
fn settle(engine: &mut Commanded<LoopEngine>) {
    heard(engine, 1);
}

/// Hold unity over a frame, ask for `target`, and return every frame mixed
/// across the two blocks — the seam between them, where a level that does not
/// ramp lands in one step, included.
fn across_a_level_change(
    engine: &mut Commanded<LoopEngine>,
    sender: &mut CommandSender,
    target: f32,
) -> Vec<f32> {
    let held = played(engine, &[1.0]);
    press(sender, Command::SetGain(target));
    let changed = played(engine, &[1.0; RAMPING_FRAMES]);

    held.into_iter().chain(changed).collect()
}

/// The largest a level moved between two frames of `mixed`.
fn largest_step(mixed: &[f32]) -> f32 {
    mixed
        .windows(2)
        .map(|pair| (pair[0] - pair[1]).abs())
        .fold(0.0, f32::max)
}

/// Play on the odd block and stop on the even one, as a player flicking
/// between the two does.
fn flicked(block: usize) -> Transport {
    if block.is_multiple_of(2) {
        Transport::Stopped
    } else {
        Transport::Playing
    }
}

/// Render blocks until a take crosses, and return the samples it crossed with.
fn crossed(engine: &mut Commanded<LoopEngine>, takes: &mut TakeReader) -> Vec<f32> {
    for _ in 0..BLOCKS_ALLOWED {
        heard(engine, 4);
        if let Some(take) = takes.claim() {
            return take.samples().collect();
        }
    }

    panic!("no take crossed");
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
        crossing(),
    );
}

#[test]
fn an_engine_answers_every_command_there_is() {
    let (writer, _position) = position_meter();
    let mut engine = LoopEngine::new(
        eight_frame_profile(),
        writer,
        waveform_meter().0,
        crossing(),
    );

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
    let layered = 0.0625;

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.125]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 1);
    for _ in 1..LoopBuffer::LAYERS {
        press(&mut sender, Command::SetTransport(Transport::Overdubbing));
        played(&mut engine, &[layered]);
        press(&mut sender, Command::SetTransport(Transport::Playing));
        heard(&mut engine, 1);
    }

    let over_the_take = layered * (LoopBuffer::LAYERS - 1) as f32;
    assert_eq!(heard(&mut engine, 1), [0.125 + over_the_take]);
}

#[test]
fn an_overdub_opened_mid_loop_lands_at_the_playhead() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.125, 0.25, 0.375, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 4), [0.125, 0.25, 0.5, 0.5]);
}

#[test]
fn an_overdub_held_across_the_loop_end_keeps_recording() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.125, 0.25, 0.375, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.0625, 0.0625, 0.0625, 0.0625]);
    played(&mut engine, &[0.125, 0.125, 0.125, 0.125]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 4), [0.25, 0.375, 0.5, 0.625]);
}

#[test]
fn an_overdub_carries_a_block_that_straddles_the_loop_end() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.0625, 0.25, 0.375]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.125]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(heard(&mut engine, 3), [0.1875, 0.25, 0.5]);
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
fn an_engine_with_nothing_recorded_publishes_no_layers() {
    let (mut engine, _sender, position) = engine();

    heard(&mut engine, 2);

    assert_eq!(position.read().depth(), 0);
}

#[test]
fn a_take_publishes_the_one_layer_it_is() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);

    assert_eq!(position.read().depth(), 1);
}

#[test]
fn an_overdub_publishes_the_layer_it_put_over_the_take() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.125]);

    assert_eq!(position.read().depth(), 2);
}

#[test]
fn a_full_stack_publishes_every_layer_it_holds() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    for _ in 1..LoopBuffer::LAYERS {
        press(&mut sender, Command::SetTransport(Transport::Playing));
        heard(&mut engine, 1);
        press(&mut sender, Command::SetTransport(Transport::Overdubbing));
        heard(&mut engine, 1);
    }

    assert_eq!(position.read().depth(), LoopBuffer::LAYERS);
}

#[test]
fn undo_publishes_the_stack_the_layer_left_behind() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.125]);
    press(&mut sender, Command::Undo);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);

    assert_eq!(position.read().depth(), 1);
}

#[test]
fn clear_publishes_a_loop_with_no_layers_left() {
    let (mut engine, mut sender, position) = engine();

    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::Clear);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    heard(&mut engine, 2);

    assert_eq!(position.read().depth(), 0);
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
    settle(&mut engine);
    press(&mut sender, Command::SetTransport(Transport::Recording));

    assert_eq!(played(&mut engine, &[0.5, 1.0]), [0.25, 0.5]);
}

#[test]
fn a_level_change_reaches_the_mixed_block_a_step_at_a_time() {
    let (mut engine, mut sender, _position) = engine_at(ramping_profile());

    let mixed = across_a_level_change(&mut engine, &mut sender, 0.25);

    assert!(
        largest_step(&mixed) <= LARGEST_STEP,
        "the level stepped by {}",
        largest_step(&mixed)
    );
    assert_eq!(mixed[RAMPING_FRAMES], 0.25);
}

#[test]
fn a_level_change_walks_the_ramp_the_device_granted() {
    let (mut engine, mut sender, _position) = engine();
    engine.prepare(ramping_config());

    let mixed = across_a_level_change(&mut engine, &mut sender, 0.25);

    assert!(
        largest_step(&mixed) <= LARGEST_STEP,
        "the level stepped by {}",
        largest_step(&mixed)
    );
    assert_eq!(mixed[RAMPING_FRAMES], 0.25);
}

#[test]
fn a_gain_that_is_not_a_number_leaves_the_level_alone() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetGain(f32::NAN));
    settle(&mut engine);

    assert_eq!(played(&mut engine, &[0.25, 0.5]), [0.25, 0.5]);
}

#[test]
fn a_gain_past_the_ceiling_is_held_at_it() {
    let (mut engine, mut sender, _position) = engine();

    press(&mut sender, Command::SetGain(Gain::CEILING + 1.0));
    settle(&mut engine);

    assert_eq!(played(&mut engine, &[1.0]), [Gain::CEILING]);
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
fn an_overdub_across_the_loop_boundary_does_not_allocate() {
    let (mut engine, mut sender, _position) = engine();
    let captured = vec![0.25; 4];
    let mut playing = vec![0.0; 4];
    press(&mut sender, Command::SetTransport(Transport::Recording));
    engine.render(&captured[..3], &mut playing[..3]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));

    let before = allocations();
    for _ in 0..LoopBuffer::LAYERS {
        engine.render(&captured, &mut playing);
    }
    let after = allocations();

    assert_eq!(after, before, "an overdub over the boundary allocated");
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
    settle(&mut engine);
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

#[test]
fn a_take_crosses_once_the_player_stops_recording() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);

    press(&mut sender, Command::SetTransport(Transport::Stopped));

    assert_eq!(crossed(&mut engine, &mut takes), [0.25, 0.5]);
}

#[test]
fn a_take_crosses_with_the_layers_the_player_laid_over_it() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.0]);

    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(crossed(&mut engine, &mut takes), [0.375, 0.5]);
}

#[test]
fn nothing_crosses_while_the_player_is_still_recording() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));

    for _ in 0..BLOCKS_ALLOWED {
        heard(&mut engine, 4);
    }

    assert!(takes.claim().is_none());
}

#[test]
fn punching_back_in_abandons_the_take_that_was_crossing() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));
    heard(&mut engine, 4);

    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    for _ in 0..BLOCKS_ALLOWED {
        heard(&mut engine, 4);
    }

    assert!(takes.claim().is_none());
}

#[test]
fn emptying_the_loop_hands_over_no_take() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Stopped));

    press(&mut sender, Command::Clear);
    for _ in 0..BLOCKS_ALLOWED {
        heard(&mut engine, 4);
    }

    assert!(takes.claim().is_none());
}

#[test]
fn undoing_a_layer_hands_the_loop_that_is_left_over() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25, 0.5]);
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    played(&mut engine, &[0.125, 0.0]);
    press(&mut sender, Command::SetTransport(Transport::Playing));
    crossed(&mut engine, &mut takes);

    press(&mut sender, Command::Undo);

    assert_eq!(crossed(&mut engine, &mut takes), [0.25, 0.5]);
}

/// Fill `profile`'s longest loop `block` frames at a time, stop, and report how
/// many frames the engine rendered before the take crossed.
fn frames_rendered_to_cross(profile: AudioProfile, block: usize) -> usize {
    let (mut engine, mut sender, mut takes) = engine_over(profile);
    press(&mut sender, Command::SetTransport(Transport::Recording));
    for _ in 0..profile.max_loop_frames() / block {
        heard(&mut engine, block);
    }
    press(&mut sender, Command::SetTransport(Transport::Stopped));

    for rendered in 0..CROSSINGS_ALLOWED {
        heard(&mut engine, block);
        if takes.claim().is_some() {
            return (rendered + 1) * block;
        }
    }

    panic!("no take crossed");
}

#[test]
fn a_take_crosses_in_the_same_frames_whatever_block_the_device_grants() {
    let profile = scaling_profile();
    let granted = profile.block_size as usize;

    assert_eq!(
        frames_rendered_to_cross(profile, granted / 2),
        frames_rendered_to_cross(profile, granted)
    );
}

#[test]
fn a_take_crosses_whole_at_a_block_shorter_than_the_profiles() {
    let profile = scaling_profile();
    let (mut engine, mut sender, mut takes) = engine_over(profile);
    press(&mut sender, Command::SetTransport(Transport::Recording));
    for _ in 0..profile.max_loop_frames() / 4 {
        played(&mut engine, &[0.25; 4]);
    }
    press(&mut sender, Command::SetTransport(Transport::Stopped));

    for _ in 0..CROSSINGS_ALLOWED {
        heard(&mut engine, 4);
        if let Some(take) = takes.claim() {
            assert_eq!(take.frames(), profile.max_loop_frames());
            return;
        }
    }

    panic!("no take crossed");
}

#[test]
fn handing_a_take_over_does_not_allocate() {
    let (mut engine, mut sender, _takes) = engine_handing_takes();
    let captured = vec![0.25; 4];
    let mut playing = vec![0.0; 4];
    press(&mut sender, Command::SetTransport(Transport::Recording));
    engine.render(&captured, &mut playing);
    press(&mut sender, Command::SetTransport(Transport::Stopped));

    let before = allocations();
    for _ in 0..BLOCKS_ALLOWED {
        engine.render(&captured, &mut playing);
    }
    let after = allocations();

    assert_eq!(after, before, "handing the take over allocated");
}

#[test]
fn a_crossing_survives_the_player_flicking_between_play_and_stop() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25; 8]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    for block in 0..BLOCKS_ALLOWED {
        press(&mut sender, Command::SetTransport(flicked(block)));
        heard(&mut engine, 4);
        if let Some(take) = takes.claim() {
            assert_eq!(take.frames(), 8);
            return;
        }
    }

    panic!("no take crossed while the player flicked between play and stop");
}

#[test]
fn an_overdub_the_stack_has_no_room_for_leaves_the_crossing_alone() {
    let (mut engine, mut sender, mut takes) = engine_handing_takes();
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut engine, &[0.25; 8]);
    for _ in 2..LoopBuffer::LAYERS {
        press(&mut sender, Command::SetTransport(Transport::Overdubbing));
        heard(&mut engine, 4);
        press(&mut sender, Command::SetTransport(Transport::Playing));
        heard(&mut engine, 4);
    }
    takes.claim();
    press(&mut sender, Command::SetTransport(Transport::Overdubbing));
    heard(&mut engine, 4);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    press(&mut sender, Command::SetTransport(Transport::Overdubbing));

    assert_eq!(crossed(&mut engine, &mut takes), [0.25; 8]);
}
