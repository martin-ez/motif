//! Moving samples between the audio callback and the application thread.
//!
//! One end of the ring lives on the real-time thread, which may not allocate,
//! lock or wait, so every operation here is wait-free: it takes what fits,
//! reports how much that was, and returns. A full ring on the producing end
//! and an empty one on the consuming end are ordinary results, not conditions
//! to retry until they clear.
//!
//! Samples are held as [`AtomicU32`] bit patterns rather than `f32` behind a
//! cell, which is what keeps the whole ring in safe code. The slots are read and
//! written relaxed; the index publication either side of them orders the data,
//! and the indices only ever grow, which is what tells a full ring from an empty
//! one. Neither end handles a count that wraps: 64 bits at an audio sample rate
//! outlast the hardware.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// Build a ring holding at most `capacity` samples, and split it into the end
/// that writes and the end that reads.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// # Panics
///
/// Panics when `capacity` is zero. Such a ring would drop every sample handed
/// to it, which is a mistake in setup rather than a condition worth reporting
/// on every block from the real-time thread.
///
/// ```
/// let (mut producer, mut consumer) = motif::audio::sample_ring(64);
/// let mut taken = [0.0; 2];
///
/// producer.write(&[1.0, 2.0]);
/// consumer.read(&mut taken);
///
/// assert_eq!(taken, [1.0, 2.0]);
/// ```
pub fn sample_ring(capacity: usize) -> (SampleProducer, SampleConsumer) {
    assert!(capacity > 0, "a sample ring holds nothing without capacity");

    let slots = (0..capacity).map(|_| AtomicU32::new(0)).collect();
    let ring = Arc::new(Ring {
        slots,
        written: AtomicUsize::new(0),
        read: AtomicUsize::new(0),
    });

    (
        SampleProducer {
            ring: Arc::clone(&ring),
        },
        SampleConsumer { ring },
    )
}

/// The writing end of a ring, held by whichever thread produces samples.
///
/// This is the end the audio callback holds when it captures input.
pub struct SampleProducer {
    ring: Arc<Ring>,
}

impl SampleProducer {
    /// Write as much of `samples` as there is room for, and report how many
    /// samples that was.
    ///
    /// A result below `samples.len()` means the ring was full and the rest were
    /// dropped: the consumer is not keeping up, and the caller decides what
    /// that means.
    pub fn write(&mut self, samples: &[f32]) -> usize {
        let taken = samples.len().min(self.vacant());
        if taken == 0 {
            return 0;
        }

        let written = self.ring.written.load(Ordering::Relaxed);
        let (front, back) = self.ring.split_at_offset(written, taken);
        for (slot, sample) in front.iter().chain(back).zip(samples) {
            slot.store(sample.to_bits(), Ordering::Relaxed);
        }
        self.ring.written.store(written + taken, Ordering::Release);

        taken
    }

    /// How many samples can be written before the ring is full.
    ///
    /// A consumer running concurrently can only make this larger, so a write of
    /// this many samples from this thread always fits.
    pub fn vacant(&self) -> usize {
        self.ring.capacity() - self.ring.occupied()
    }

    /// The most samples the ring can hold at once.
    pub fn capacity(&self) -> usize {
        self.ring.capacity()
    }
}

/// The reading end of a ring, held by whichever thread consumes samples.
///
/// This is the end the application thread holds when it drains captured input.
pub struct SampleConsumer {
    ring: Arc<Ring>,
}

impl SampleConsumer {
    /// Fill as much of `out` as there are samples for, and report how many
    /// samples that was.
    ///
    /// A result below `out.len()` means the ring ran dry, which for an output
    /// path is an underrun and for an input path is simply a block that has not
    /// been captured yet. The rest of `out` is left as it was.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let taken = out.len().min(self.available());
        if taken == 0 {
            return 0;
        }

        let read = self.ring.read.load(Ordering::Relaxed);
        let (front, back) = self.ring.split_at_offset(read, taken);
        for (slot, sample) in front.iter().chain(back).zip(out.iter_mut()) {
            *sample = f32::from_bits(slot.load(Ordering::Relaxed));
        }
        self.ring.read.store(read + taken, Ordering::Release);

        taken
    }

    /// Drop up to `count` samples without reading them, and report how many
    /// that was.
    ///
    /// A result below `count` means the ring ran dry first. Nothing is copied,
    /// so this is a place to put samples that are known to be stale rather than
    /// a faster [`read`](Self::read).
    ///
    /// ```
    /// let (mut producer, mut consumer) = motif::audio::sample_ring(4);
    /// let mut taken = [0.0; 2];
    ///
    /// producer.write(&[1.0, 2.0, 3.0, 4.0]);
    /// consumer.skip(2);
    /// consumer.read(&mut taken);
    ///
    /// assert_eq!(taken, [3.0, 4.0]);
    /// ```
    pub fn skip(&mut self, count: usize) -> usize {
        let dropped = count.min(self.available());
        if dropped == 0 {
            return 0;
        }

        let read = self.ring.read.load(Ordering::Relaxed);
        self.ring.read.store(read + dropped, Ordering::Release);

        dropped
    }

    /// How many samples are waiting to be read.
    ///
    /// A producer running concurrently can only make this larger, so a read of
    /// this many samples from this thread always succeeds.
    pub fn available(&self) -> usize {
        self.ring.occupied()
    }

    /// The most samples the ring can hold at once.
    pub fn capacity(&self) -> usize {
        self.ring.capacity()
    }
}

struct Ring {
    slots: Box<[AtomicU32]>,
    written: AtomicUsize,
    read: AtomicUsize,
}

impl Ring {
    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn occupied(&self) -> usize {
        let read = self.read.load(Ordering::Acquire);
        let written = self.written.load(Ordering::Acquire);
        written - read
    }

    fn split_at_offset(&self, position: usize, length: usize) -> (&[AtomicU32], &[AtomicU32]) {
        let start = position % self.capacity();
        let contiguous = length.min(self.capacity() - start);
        (
            &self.slots[start..start + contiguous],
            &self.slots[..length - contiguous],
        )
    }
}
