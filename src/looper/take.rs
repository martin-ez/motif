//! Handing a finished take to the thread that analyses it.
//!
//! The samples themselves have to cross this time: an analyser wants the loop
//! rather than a summary of it. What makes that safe on a callback that may not
//! allocate or block is that the memory is already there and the crossing ends
//! in an exchange of slots rather than a copy — three of them, so the writer
//! always has one to fill and the reader can hold one for as long as its
//! analysis takes.
//!
//! A take is finished exactly once, when the player leaves a capturing state,
//! and it is swept across over a fixed span of frames rather than in a single
//! pass: a whole loop copied inside one callback is a spike no deadline
//! survives. Samples travel as bits in atomics, as the waveform's buckets do.

use std::array;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::device::AudioProfile;
use crate::seq::Bars;

use super::LoopBuffer;

const SLOT_COUNT: usize = 3;
const CROSSING_BLOCK_COUNT: usize = 64;
const UNREAD: usize = SLOT_COUNT;
const UNCOUNTED: u32 = 0;

struct Shared {
    slots: [Box<[AtomicU32]>; SLOT_COUNT],
    frames: [AtomicUsize; SLOT_COUNT],
    bars: [AtomicU32; SLOT_COUNT],
    published: AtomicUsize,
}

#[derive(Clone, Copy)]
struct Crossing {
    frames: usize,
    cursor: usize,
}

/// Build a take handoff, and split it into the end that publishes and the end
/// that reads.
///
/// Three slots of the longest loop `profile` allows — 6.1 MB a slot at
/// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET) — allocated
/// here and never again, so this belongs in setup. Three is what lets the
/// writer fill one while the reader holds another.
///
/// # Panics
///
/// Panics on a profile with no loop to record, or no block to cross it in.
///
/// ```
/// use motif::device::DeviceProfile;
/// use motif::looper::{LoopBuffer, TakeWriter, take_handoff};
///
/// let profile = DeviceProfile::TARGET.audio;
/// let (mut writer, mut reader) = take_handoff(profile);
/// let mut buffer = LoopBuffer::for_profile(profile);
/// buffer.record(&[0.25, 0.5]);
///
/// writer.begin(&buffer, None);
/// for _ in 0..TakeWriter::CROSSING_BLOCKS {
///     writer.advance(&buffer, profile.block_size as usize);
/// }
///
/// let take = reader.claim().expect("a finished take crossed");
/// assert_eq!(take.samples().collect::<Vec<_>>(), [0.25, 0.5]);
/// ```
pub fn take_handoff(profile: AudioProfile) -> (TakeWriter, TakeReader) {
    let capacity = profile.max_loop_frames();
    let block = profile.block_size as usize;
    assert!(capacity > 0, "a handoff carries no take without frames");
    assert!(block > 0, "a take crosses nothing block by block");

    let shared = Arc::new(Shared {
        slots: array::from_fn(|_| (0..capacity).map(|_| AtomicU32::new(0)).collect()),
        frames: [const { AtomicUsize::new(0) }; SLOT_COUNT],
        bars: [const { AtomicU32::new(UNCOUNTED) }; SLOT_COUNT],
        published: AtomicUsize::new(SLOT_COUNT - 1),
    });

    (
        TakeWriter {
            shared: Arc::clone(&shared),
            scratch: vec![0.0; capacity.div_ceil(TakeWriter::CROSSING_BLOCKS)].into_boxed_slice(),
            block,
            writing: 0,
            crossing: None,
        },
        TakeReader { shared, reading: 1 },
    )
}

/// The publishing end of a take handoff, held by whichever thread owns the
/// loop.
///
/// This is the end the audio callback holds.
pub struct TakeWriter {
    shared: Arc<Shared>,
    scratch: Box<[f32]>,
    block: usize,
    writing: usize,
    crossing: Option<Crossing>,
}

impl TakeWriter {
    /// How many blocks of the profile's size a take is spread across.
    ///
    /// A whole loop is megabytes, and copying it inside one callback is a spike
    /// no deadline survives. Spreading it over that span of frames rather than
    /// a count of callbacks is what makes every take cross in the same
    /// wall-clock, about a third of a second at
    /// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET),
    /// whatever block the device granted. The cost is a share that grows with
    /// the loop rather than with the block.
    pub const CROSSING_BLOCKS: usize = CROSSING_BLOCK_COUNT;

    /// Begin handing over the loop `buffer` holds, dropping whatever crossing
    /// was in flight.
    ///
    /// The length is taken now, so what crosses is the take as it stood at the
    /// boundary the player just left. A loop of no frames begins nothing, and
    /// one longer than the handoff was built for crosses as much as fits.
    pub fn begin(&mut self, buffer: &LoopBuffer, bars: Option<Bars>) {
        let frames = buffer.len().min(self.shared.slots[self.writing].len());

        self.shared.bars[self.writing]
            .store(bars.map_or(UNCOUNTED, Bars::to_bits), Ordering::Relaxed);
        self.crossing = (frames > 0).then_some(Crossing { frames, cursor: 0 });
    }

    /// Carry the crossing forward by the share `handed` frames of block buy,
    /// and report whether any of it is still to cross.
    ///
    /// The share follows those frames against the block the profile states, so
    /// a shorter block carries less of the take in each of the more callbacks
    /// it makes and a crossing costs the same wall-clock either way. It is
    /// capped at the scratch a whole block's share was allocated in, so a
    /// longer block is bounded rather than a panic. Nothing is published until
    /// the last share lands, and with none in flight the answer is `false`.
    pub fn advance(&mut self, buffer: &LoopBuffer, handed: usize) -> bool {
        let Some(Crossing { frames, cursor }) = self.crossing else {
            return false;
        };

        let share = (frames - cursor).min(self.share_of(frames, handed));
        let crossing = &mut self.scratch[..share];
        crossing.fill(0.0);
        buffer.mix_into(crossing, cursor);

        let slot = &self.shared.slots[self.writing][cursor..cursor + share];
        for (cell, sample) in slot.iter().zip(crossing) {
            cell.store(sample.to_bits(), Ordering::Relaxed);
        }

        self.crossing = (cursor + share < frames).then_some(Crossing {
            frames,
            cursor: cursor + share,
        });
        if self.crossing.is_none() {
            self.publish(frames);
        }

        self.crossing.is_some()
    }

    /// Drop the crossing in flight, leaving the last take that finished one
    /// where it is.
    ///
    /// What the player does to the loop mid-crossing — punching back in,
    /// undoing a layer, emptying it — leaves the take being swept out of date
    /// rather than wrong, so it is dropped rather than published.
    pub fn abandon(&mut self) {
        self.crossing = None;
    }

    fn share_of(&self, frames: usize, handed: usize) -> usize {
        frames
            .div_ceil(Self::CROSSING_BLOCKS)
            .saturating_mul(handed)
            .div_ceil(self.block)
            .min(self.scratch.len())
    }

    fn publish(&mut self, frames: usize) {
        self.shared.frames[self.writing].store(frames, Ordering::Relaxed);
        let read = self
            .shared
            .published
            .swap(self.writing + UNREAD, Ordering::AcqRel);

        self.writing = read % SLOT_COUNT;
    }
}

/// The reading end of a take handoff, held by whichever thread analyses the
/// loop.
///
/// This is the end the application thread holds.
pub struct TakeReader {
    shared: Arc<Shared>,
    reading: usize,
}

impl TakeReader {
    /// The newest finished take, or nothing where none has finished since the
    /// last was taken.
    ///
    /// What comes back is the reader's until it is dropped: the writer
    /// publishes past it rather than into it, so an analysis pass may run as
    /// long as it likes. Takes finishing while one is held replace each other,
    /// so what a long pass finds waiting is the newest rather than the next.
    pub fn claim(&mut self) -> Option<FinishedTake<'_>> {
        if self.shared.published.load(Ordering::Acquire) < UNREAD {
            return None;
        }

        let unread = self.shared.published.swap(self.reading, Ordering::AcqRel);
        self.reading = unread - UNREAD;
        let frames = self.shared.frames[self.reading].load(Ordering::Relaxed);

        Some(FinishedTake {
            samples: &self.shared.slots[self.reading][..frames],
            bars: Bars::from_bits(self.shared.bars[self.reading].load(Ordering::Relaxed)),
        })
    }
}

/// A finished take, held by the thread reading it.
///
/// The samples are the loop's layers summed — what the player heard, and what
/// harmony is inferred from. They are read out one at a time rather than
/// borrowed as a slice, because the storage they crossed in is shared with the
/// callback.
pub struct FinishedTake<'a> {
    samples: &'a [AtomicU32],
    bars: Option<Bars>,
}

impl FinishedTake<'_> {
    /// How many frames the take holds.
    pub fn frames(&self) -> usize {
        self.samples.len()
    }

    /// How the player said the take divides, where they said.
    ///
    /// Nothing infers it and nothing defaults it: a take nobody counted comes
    /// back uncounted, because a guessed count tracks worse than no count.
    pub const fn bars(&self) -> Option<Bars> {
        self.bars
    }

    /// The take's samples, one a frame, from its first to its last.
    pub fn samples(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.samples
            .iter()
            .map(|cell| f32::from_bits(cell.load(Ordering::Relaxed)))
    }
}
