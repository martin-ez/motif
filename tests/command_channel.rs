//! The queue that carries commands from the application thread to the audio
//! callback.
//!
//! The application thread holds one end and the callback the other, so the
//! facts worth stating are that a command crosses intact and in order, that a
//! command sent before a block is applied by that block, that a full queue
//! refuses the send rather than swallowing it, and that neither end allocates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::{black_box, spin_loop};
use std::thread;
use std::time::{Duration, Instant};

use motif::audio::{Command, SendError, command_channel};
use motif::looper::Transport;
use motif::seq::Bars;

/// How long a concurrent test goes without moving a single command before it
/// decides the queue has stalled. It bounds a run of fruitless attempts rather
/// than the test as a whole, so a slow machine only makes the test slow, while
/// a queue that has stopped moving commands fails rather than hangs.
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
    black_box(Vec::<Command>::with_capacity(4));
    let after = allocations();

    assert!(after > before, "the counter is not wired to the allocator");
}

#[test]
fn a_command_arrives_at_the_receiver() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::SetMuted(true))
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::SetMuted(true)));
}

#[test]
fn commands_arrive_in_the_order_they_were_sent() {
    let (mut sender, mut receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    sender
        .send(Command::SetMuted(true))
        .expect("a queue with room accepts a command");

    let received: Vec<Command> = receiver.drain().collect();

    assert_eq!(
        received,
        [
            Command::SetTransport(Transport::Recording),
            Command::SetMuted(true)
        ]
    );
}

#[test]
fn a_transport_command_carries_the_state_it_was_given() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::SetTransport(Transport::Overdubbing))
        .expect("a queue with room accepts a command");

    assert_eq!(
        receiver.recv(),
        Some(Command::SetTransport(Transport::Overdubbing))
    );
}

#[test]
fn every_transport_state_survives_the_crossing() {
    let (mut sender, mut receiver) = command_channel(8);
    let states = [
        Transport::Idle,
        Transport::Recording,
        Transport::Playing,
        Transport::Overdubbing,
        Transport::Stopped,
    ];

    for state in states {
        sender
            .send(Command::SetTransport(state))
            .expect("a queue with room accepts a command");
    }

    let received: Vec<Command> = receiver.drain().collect();
    assert_eq!(received, states.map(Command::SetTransport));
}

#[test]
fn an_undo_command_crosses_the_boundary() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::Undo)
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::Undo));
}

#[test]
fn a_clear_command_crosses_the_boundary() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::Clear)
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::Clear));
}

#[test]
fn a_mute_command_carries_the_state_it_was_given() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::SetMuted(false))
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::SetMuted(false)));
}

#[test]
fn a_gain_command_carries_its_value_unchanged() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::SetGain(0.375))
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::SetGain(0.375)));
}

#[test]
fn a_gain_survives_the_crossing_at_the_edges_of_its_range() {
    let (mut sender, mut receiver) = command_channel(8);
    let edges = [0.0, -1.0, f32::MIN_POSITIVE, f32::MAX];

    for gain in edges {
        sender
            .send(Command::SetGain(gain))
            .expect("a queue with room accepts a command");
    }

    let received: Vec<Command> = receiver.drain().collect();
    assert_eq!(received, edges.map(Command::SetGain));
}

#[test]
fn a_bar_count_carries_both_of_its_halves() {
    let (mut sender, mut receiver) = command_channel(8);
    let counted = Bars::of(7, 5);

    sender
        .send(Command::SetBars(counted))
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::SetBars(counted)));
}

#[test]
fn a_take_nobody_counted_crosses_as_uncounted() {
    let (mut sender, mut receiver) = command_channel(8);

    sender
        .send(Command::SetBars(None))
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.recv(), Some(Command::SetBars(None)));
}

#[test]
fn a_bar_count_survives_the_crossing_at_the_edges_of_its_range() {
    let (mut sender, mut receiver) = command_channel(8);
    let edges = [Bars::of(1, 1), Bars::of(Bars::MOST, Bars::MOST)];

    for bars in edges {
        sender
            .send(Command::SetBars(bars))
            .expect("a queue with room accepts a command");
    }

    let received: Vec<Command> = receiver.drain().collect();
    assert_eq!(received, edges.map(Command::SetBars));
}

#[test]
fn a_bar_count_missing_one_of_its_halves_is_refused() {
    let uncounted = Command::SetBars(None).to_bits();
    let one_bar_of_one = Command::SetBars(Bars::of(1, 1)).to_bits();

    assert_eq!(Command::from_bits(uncounted + 1), None);
    assert_eq!(Command::from_bits(one_bar_of_one - 1), None);
}

#[test]
fn every_command_round_trips_through_its_bits() {
    let commands = [
        Command::SetTransport(Transport::Overdubbing),
        Command::SetMuted(true),
        Command::SetGain(0.375),
        Command::SetBars(Bars::of(7, 5)),
        Command::SetBars(None),
        Command::Undo,
        Command::Clear,
    ];

    let returned: Vec<Option<Command>> = commands
        .into_iter()
        .map(|command| Command::from_bits(command.to_bits()))
        .collect();

    assert_eq!(returned, commands.map(Some));
}

#[test]
fn a_tag_naming_no_command_is_refused() {
    assert_eq!(Command::from_bits(u64::MAX), None);
}

#[test]
fn a_transport_state_that_does_not_exist_is_refused() {
    let states = [
        Transport::Idle,
        Transport::Recording,
        Transport::Playing,
        Transport::Overdubbing,
        Transport::Stopped,
    ];
    let last = states
        .map(|state| Command::SetTransport(state).to_bits())
        .into_iter()
        .max()
        .expect("the transport has states");

    assert_eq!(Command::from_bits(last + 1), None);
}

#[test]
fn a_flag_that_is_neither_set_nor_clear_is_refused() {
    let set = Command::SetMuted(true).to_bits();

    assert_eq!(Command::from_bits(set + 1), None);
}

#[test]
fn a_command_carrying_no_payload_refuses_one() {
    let refused =
        [Command::Undo, Command::Clear].map(|command| Command::from_bits(command.to_bits() + 1));

    assert_eq!(refused, [None, None]);
}

#[test]
fn a_receiver_with_nothing_pending_takes_nothing() {
    let (_sender, mut receiver) = command_channel(8);

    assert_eq!(receiver.recv(), None);
}

#[test]
fn a_full_queue_refuses_the_send() {
    let (mut sender, _receiver) = command_channel(2);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    sender
        .send(Command::SetTransport(Transport::Playing))
        .expect("a queue with room accepts a command");

    assert_eq!(sender.send(Command::SetMuted(true)), Err(SendError::Full));
}

#[test]
fn a_refused_command_is_not_delivered() {
    let (mut sender, mut receiver) = command_channel(2);
    let _ = sender.send(Command::SetTransport(Transport::Recording));
    let _ = sender.send(Command::SetTransport(Transport::Playing));
    let _ = sender.send(Command::SetMuted(true));

    let received: Vec<Command> = receiver.drain().collect();

    assert_eq!(
        received,
        [
            Command::SetTransport(Transport::Recording),
            Command::SetTransport(Transport::Playing)
        ]
    );
}

#[test]
fn receiving_frees_the_slot_it_took() {
    let (mut sender, mut receiver) = command_channel(2);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    sender
        .send(Command::SetTransport(Transport::Playing))
        .expect("a queue with room accepts a command");

    receiver.recv();

    assert_eq!(sender.send(Command::SetMuted(true)), Ok(()));
}

#[test]
fn commands_survive_wrapping_around_the_end_of_the_queue() {
    let (mut sender, mut receiver) = command_channel(2);
    sender
        .send(Command::SetGain(1.0))
        .expect("a queue with room accepts a command");
    receiver.recv();

    sender
        .send(Command::SetGain(2.0))
        .expect("a queue with room accepts a command");
    sender
        .send(Command::SetGain(3.0))
        .expect("a queue with room accepts a command");

    let received: Vec<Command> = receiver.drain().collect();
    assert_eq!(received, [Command::SetGain(2.0), Command::SetGain(3.0)]);
}

#[test]
fn a_queue_that_is_not_a_power_of_two_wraps_cleanly() {
    let (mut sender, mut receiver) = command_channel(3);
    let mut received = Vec::new();

    for round in 0..64 {
        sender
            .send(Command::SetGain(round as f32))
            .expect("a drained queue has room");
        received.extend(receiver.drain());
    }

    let expected: Vec<Command> = (0..64)
        .map(|round| Command::SetGain(round as f32))
        .collect();
    assert_eq!(received, expected);
}

#[test]
fn a_drain_takes_everything_that_was_pending() {
    let (mut sender, mut receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    sender
        .send(Command::SetGain(0.5))
        .expect("a queue with room accepts a command");

    let received: Vec<Command> = receiver.drain().collect();

    assert_eq!(received.len(), 2);
}

#[test]
fn a_drain_leaves_the_queue_empty() {
    let (mut sender, mut receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");

    receiver.drain().count();

    assert_eq!(receiver.pending(), 0);
}

#[test]
fn a_drain_of_an_empty_queue_takes_nothing() {
    let (_sender, mut receiver) = command_channel(8);

    assert_eq!(receiver.drain().count(), 0);
}

#[test]
fn a_drain_stops_at_what_was_pending_when_it_started() {
    let (mut sender, mut receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");

    let mut drain = receiver.drain();
    let first = drain.next();
    sender
        .send(Command::SetMuted(true))
        .expect("a queue with room accepts a command");
    let rest: Vec<Command> = drain.collect();

    assert_eq!(
        (first, rest),
        (
            Some(Command::SetTransport(Transport::Recording)),
            Vec::new()
        )
    );
}

#[test]
fn a_command_sent_after_a_drain_is_waiting_for_the_next_one() {
    let (mut sender, mut receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    receiver.drain().count();

    sender
        .send(Command::SetMuted(true))
        .expect("a queue with room accepts a command");

    let received: Vec<Command> = receiver.drain().collect();
    assert_eq!(received, [Command::SetMuted(true)]);
}

#[test]
fn a_queue_reports_the_capacity_it_was_built_with() {
    let (sender, receiver) = command_channel(64);

    assert_eq!((sender.capacity(), receiver.capacity()), (64, 64));
}

#[test]
fn vacant_slots_fall_as_the_queue_fills() {
    let (mut sender, _receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");

    assert_eq!(sender.vacant(), 7);
}

#[test]
fn pending_commands_rise_as_the_queue_fills() {
    let (mut sender, receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    sender
        .send(Command::SetMuted(true))
        .expect("a queue with room accepts a command");

    assert_eq!(receiver.pending(), 2);
}

#[test]
#[should_panic(expected = "capacity")]
fn a_queue_with_no_capacity_is_refused_at_setup() {
    command_channel(0);
}

#[test]
fn a_queue_of_one_command_still_carries_them_one_at_a_time() {
    let (mut sender, mut receiver) = command_channel(1);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");
    receiver.recv();

    sender
        .send(Command::SetTransport(Transport::Playing))
        .expect("a drained queue has room");

    assert_eq!(
        receiver.recv(),
        Some(Command::SetTransport(Transport::Playing))
    );
}

#[test]
fn a_send_error_describes_itself() {
    assert_eq!(SendError::Full.to_string(), "the command queue is full");
}

#[test]
fn every_command_survives_a_concurrent_round_trip() {
    const COMMANDS: usize = 100_000;

    let (mut sender, mut receiver) = command_channel(64);
    let mut received = Vec::with_capacity(COMMANDS);

    thread::scope(|scope| {
        scope.spawn(move || {
            let mut deadline = Instant::now() + PATIENCE;
            let mut position = 0;
            while position < COMMANDS {
                if sender.send(Command::SetGain(position as f32)).is_ok() {
                    deadline = Instant::now() + PATIENCE;
                    position += 1;
                } else {
                    assert!(Instant::now() < deadline, "the queue never made room");
                    spin_loop();
                }
            }
        });

        let mut deadline = Instant::now() + PATIENCE;
        while received.len() < COMMANDS {
            let before = received.len();
            received.extend(receiver.drain());
            if received.len() == before {
                assert!(Instant::now() < deadline, "the queue never delivered");
                spin_loop();
            } else {
                deadline = Instant::now() + PATIENCE;
            }
        }
    });

    let expected: Vec<Command> = (0..COMMANDS)
        .map(|position| Command::SetGain(position as f32))
        .collect();
    assert_eq!(received, expected);
}

#[test]
fn neither_end_allocates() {
    let (mut sender, mut receiver) = command_channel(8);

    let before = allocations();
    for _ in 0..8 {
        let _ = sender.send(Command::SetGain(0.5));
        let _ = receiver.recv();
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn neither_end_allocates_when_the_queue_is_full_or_empty() {
    let (mut sender, mut receiver) = command_channel(2);

    let before = allocations();
    for _ in 0..4 {
        let _ = sender.send(Command::SetMuted(true));
    }
    for _ in 0..4 {
        let _ = receiver.recv();
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn draining_does_not_allocate() {
    let (mut sender, mut receiver) = command_channel(8);
    sender
        .send(Command::SetTransport(Transport::Recording))
        .expect("a queue with room accepts a command");

    let before = allocations();
    receiver.drain().count();
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn crossing_the_looper_commands_does_not_allocate() {
    let (mut sender, mut receiver) = command_channel(8);

    let before = allocations();
    for command in [
        Command::SetTransport(Transport::Overdubbing),
        Command::Undo,
        Command::Clear,
    ] {
        let _ = sender.send(command);
        let _ = receiver.recv();
    }
    let after = allocations();

    assert_eq!(after, before);
}

#[test]
fn the_encoding_does_not_allocate() {
    let before = allocations();
    for command in [
        Command::SetTransport(Transport::Recording),
        Command::SetMuted(true),
        Command::SetGain(0.5),
        Command::Undo,
        Command::Clear,
    ] {
        let _ = Command::from_bits(command.to_bits());
    }
    let after = allocations();

    assert_eq!(after, before);
}
