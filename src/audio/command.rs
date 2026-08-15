//! Changing what the audio callback does, from the application thread.
//!
//! The receiving end lives on the real-time thread, which may not allocate,
//! lock or wait, so a command crosses as a value written into a slot that is
//! already there — data rather than a closure, since a closure that captures is
//! an allocation on one thread and a vtable dispatch on the other.
//!
//! A command that sets something carries the state to be in rather than a
//! change to it: [`Command::SetTransport`] carries the whole of [`Transport`],
//! not a flag derived from it. A toggle means something different depending on
//! how many of its predecessors arrived, so one refused send would leave the
//! two ends disagreeing for good.
//!
//! Each slot is an [`AtomicU64`] bit pattern, which is what keeps the queue in
//! safe code; a pattern naming no command is discarded rather than guessed at.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::looper::Transport;
use crate::seq::Bars;

const TRANSPORT: u64 = 0;
const MUTED: u64 = 1;
const GAIN: u64 = 2;
const UNDO: u64 = 3;
const CLEAR: u64 = 4;
const BARS: u64 = 5;
const TAG_SHIFT: u32 = 32;
const NO_PAYLOAD: u32 = 0;

fn bars_from_bits(payload: u32) -> Option<Option<Bars>> {
    if payload == NO_PAYLOAD {
        return Some(None);
    }

    Bars::from_bits(payload).map(Some)
}

/// A change to what the callback does.
///
/// The set is closed, and deliberately not `#[non_exhaustive]`: a `match` in
/// the callback stops compiling when a command is added, which is a better
/// outcome than a command nothing applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Put the transport in a state: open a take or a layer, play the loop, or
    /// halt it.
    SetTransport(Transport),
    /// Mute the output, or unmute it.
    SetMuted(bool),
    /// Set the gain applied to the input, as a linear multiplier where `1.0` is
    /// unity and `0.0` is silence.
    SetGain(f32),
    /// State how the take divides, or that nobody has stated it.
    ///
    /// A count nobody gave is carried as one nobody gave: a guessed one tracks
    /// worse than none at all, so there is no default to fall back on.
    SetBars(Option<Bars>),
    /// Take the most recent overdub off the loop, keeping the take under it.
    Undo,
    /// Empty the loop, back to nothing recorded.
    Clear,
}

impl Command {
    /// The bit pattern this command crosses the queue as: a tag naming the
    /// command, and a payload carrying what it says.
    #[must_use]
    pub fn to_bits(self) -> u64 {
        let (tag, payload) = match self {
            Self::SetTransport(transport) => (TRANSPORT, transport_bits(transport)),
            Self::SetMuted(muted) => (MUTED, u32::from(muted)),
            Self::SetGain(gain) => (GAIN, gain.to_bits()),
            Self::SetBars(bars) => (BARS, bars.map_or(NO_PAYLOAD, Bars::to_bits)),
            Self::Undo => (UNDO, NO_PAYLOAD),
            Self::Clear => (CLEAR, NO_PAYLOAD),
        };
        (tag << TAG_SHIFT) | u64::from(payload)
    }

    /// The command a bit pattern names, or `None` where it names none.
    ///
    /// Exactly what [`to_bits`](Self::to_bits) produces is accepted: a tag no
    /// command carries, or a payload no command of that tag would ever hold, is
    /// refused rather than guessed at. The callback applying the result is the
    /// reason — a guess there is a state the player never asked for.
    #[must_use]
    pub fn from_bits(bits: u64) -> Option<Self> {
        let payload = bits as u32;
        match bits >> TAG_SHIFT {
            TRANSPORT => transport_from_bits(payload).map(Self::SetTransport),
            MUTED if payload <= 1 => Some(Self::SetMuted(payload != 0)),
            GAIN => Some(Self::SetGain(f32::from_bits(payload))),
            BARS => bars_from_bits(payload).map(Self::SetBars),
            UNDO if payload == NO_PAYLOAD => Some(Self::Undo),
            CLEAR if payload == NO_PAYLOAD => Some(Self::Clear),
            _ => None,
        }
    }
}

fn transport_bits(transport: Transport) -> u32 {
    match transport {
        Transport::Idle => 0,
        Transport::Recording => 1,
        Transport::Playing => 2,
        Transport::Overdubbing => 3,
        Transport::Stopped => 4,
    }
}

fn transport_from_bits(payload: u32) -> Option<Transport> {
    match payload {
        0 => Some(Transport::Idle),
        1 => Some(Transport::Recording),
        2 => Some(Transport::Playing),
        3 => Some(Transport::Overdubbing),
        4 => Some(Transport::Stopped),
        _ => None,
    }
}

/// Why a command could not be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SendError {
    /// The queue has no room. The command was not queued, and the sender
    /// decides whether to retry it or to drop it.
    Full,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let described = match self {
            Self::Full => "the command queue is full",
        };
        f.write_str(described)
    }
}

impl std::error::Error for SendError {}

/// Build a queue holding at most `capacity` commands, and split it into the end
/// that sends and the end that receives.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// # Panics
///
/// Panics when `capacity` is zero. Such a queue would refuse every command ever
/// sent, which is a mistake in setup rather than a condition worth reporting on
/// every send.
///
/// ```
/// use motif::audio::{Command, SendError, command_channel};
///
/// let (mut sender, mut receiver) = command_channel(16);
/// sender.send(Command::SetGain(0.5))?;
///
/// let applied: Vec<Command> = receiver.drain().collect();
/// assert_eq!(applied, [Command::SetGain(0.5)]);
/// # Ok::<(), SendError>(())
/// ```
pub fn command_channel(capacity: usize) -> (CommandSender, CommandReceiver) {
    assert!(
        capacity > 0,
        "a command queue holds nothing without capacity"
    );

    let slots = (0..capacity).map(|_| AtomicU64::new(0)).collect();
    let queue = Arc::new(Queue {
        slots,
        sent: AtomicUsize::new(0),
        received: AtomicUsize::new(0),
    });

    (
        CommandSender {
            queue: Arc::clone(&queue),
        },
        CommandReceiver { queue },
    )
}

/// The sending end of a command queue, held by the application thread.
pub struct CommandSender {
    queue: Arc<Queue>,
}

impl CommandSender {
    /// Queue `command` for the callback to apply on its next block.
    ///
    /// # Errors
    ///
    /// Returns [`SendError::Full`] when the queue has no room, leaving the
    /// command unqueued. A full queue is reported rather than absorbed because
    /// a control the user moved and the engine never heard is worse than one
    /// the application knows it has to send again.
    pub fn send(&mut self, command: Command) -> Result<(), SendError> {
        if self.vacant() == 0 {
            return Err(SendError::Full);
        }

        let sent = self.queue.sent.load(Ordering::Relaxed);
        self.queue
            .slot(sent)
            .store(command.to_bits(), Ordering::Relaxed);
        self.queue.sent.store(sent + 1, Ordering::Release);

        Ok(())
    }

    /// How many commands can be sent before the queue is full.
    ///
    /// A receiver running concurrently can only make this larger, so a send
    /// from this thread while this is non-zero always fits.
    pub fn vacant(&self) -> usize {
        self.queue.capacity() - self.queue.pending()
    }
}

/// The receiving end of a command queue, held by the audio callback.
pub struct CommandReceiver {
    queue: Arc<Queue>,
}

impl CommandReceiver {
    /// Take the oldest command waiting, or `None` when none is.
    ///
    /// An empty queue is the ordinary case — most blocks change nothing — so it
    /// is a result to carry on from, never a condition to wait for.
    pub fn recv(&mut self) -> Option<Command> {
        if self.pending() == 0 {
            return None;
        }

        let received = self.queue.received.load(Ordering::Relaxed);
        let bits = self.queue.slot(received).load(Ordering::Relaxed);
        self.queue.received.store(received + 1, Ordering::Release);

        Command::from_bits(bits)
    }

    /// Take every command that was waiting when the drain began, oldest first.
    ///
    /// The count is fixed when the iterator is made, so a sender running
    /// concurrently cannot extend the run: the callback applies what arrived
    /// before its block and nothing later, and the loop is bounded before it
    /// starts rather than by whatever the other thread does next.
    pub fn drain(&mut self) -> impl Iterator<Item = Command> {
        let mut remaining = self.pending();
        std::iter::from_fn(move || {
            remaining = remaining.checked_sub(1)?;
            self.recv()
        })
    }

    /// How many commands are waiting to be received.
    ///
    /// A sender running concurrently can only make this larger, so this many
    /// receives from this thread always succeed.
    pub fn pending(&self) -> usize {
        self.queue.pending()
    }
}

struct Queue {
    slots: Box<[AtomicU64]>,
    sent: AtomicUsize,
    received: AtomicUsize,
}

impl Queue {
    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn pending(&self) -> usize {
        let received = self.received.load(Ordering::Acquire);
        let sent = self.sent.load(Ordering::Acquire);
        sent - received
    }

    fn slot(&self, position: usize) -> &AtomicU64 {
        &self.slots[position % self.capacity()]
    }
}
