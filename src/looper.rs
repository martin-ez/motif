//! What the player is doing with the loop, and where the loop is kept.
//!
//! [`Transport`] is the state a player drives with record, play and stop;
//! [`LoopBuffer`] holds the samples those states govern.
//!
//! [`LoopBuffer`] is sized from the device profile and allocated before the
//! stream starts, so the longest loop a player can capture is a constraint the
//! machine states rather than an accident of how much memory happens to be
//! free. On hardware it is the former either way, and a buffer that grew to
//! meet a long take would have to allocate on the thread that may not. That is
//! why layers are a fixed stack of [`LoopBuffer::LAYERS`] rather than a list
//! that grows with each overdub: the depth is a number the machine states.
//!
//! Layers are summed when the loop is read rather than as it is recorded, which
//! is what makes [`LoopBuffer::undo`] a change to one index. Kept as a running
//! mix, undo would have to subtract a layer back out — which does not restore
//! the samples exactly, since a float sum is not reversible — or re-sum what is
//! left, which is the whole loop's work inside one block. Summing on the way out
//! costs a layer's addition per frame played, bounded by the stated depth.
//!
//! One sample per frame, as the ring across the audio boundary carries them: a
//! channel layout belongs to a device, and nothing this side of the callback
//! should have to know one.

use crate::device::AudioProfile;

mod page;
mod position;
mod transport;

pub use page::LooperPage;
pub use position::{LoopPosition, PositionReader, PositionWriter, position_meter};
pub use transport::Transport;

const LAYER_COUNT: usize = 8;

/// The samples of a loop, in storage that is allocated once and never grows.
pub struct LoopBuffer {
    layers: Box<[f32]>,
    written: [usize; LAYER_COUNT],
    depth: usize,
    open: Option<usize>,
}

impl LoopBuffer {
    /// How many layers a loop is built from: the take, and seven overdubs over
    /// it.
    ///
    /// Every layer is a buffer of the profile's longest loop, allocated at
    /// setup and never released, so depth is bought with memory. At
    /// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET) — 48 kHz
    /// for 32 seconds — that is 6.1 MB a layer and 49 MB for the stack.
    pub const LAYERS: usize = LAYER_COUNT;

    /// Allocate a buffer for the longest loop `profile` allows, at every layer.
    ///
    /// The storage is allocated here and never again, so this belongs in
    /// setup, before the stream starts.
    ///
    /// # Panics
    ///
    /// Panics when the profile's longest loop is no frames at all. Such a
    /// buffer could hold no loop, which is a mistake in setup rather than a
    /// condition worth reporting on every block from the real-time thread.
    ///
    /// ```
    /// use motif::device::DeviceProfile;
    /// use motif::looper::LoopBuffer;
    ///
    /// let profile = DeviceProfile::TARGET.audio;
    /// let mut captured = LoopBuffer::for_profile(profile);
    ///
    /// captured.record(&[0.25, 0.5]);
    ///
    /// let mut heard = [0.0; 2];
    /// captured.mix_into(&mut heard, 0);
    ///
    /// assert_eq!(heard, [0.25, 0.5]);
    /// assert_eq!(captured.capacity(), profile.max_loop_frames());
    /// ```
    pub fn for_profile(profile: AudioProfile) -> Self {
        let capacity = profile.max_loop_frames();
        assert!(capacity > 0, "a loop buffer holds no loop without frames");

        Self {
            layers: vec![0.0; capacity.saturating_mul(Self::LAYERS)].into_boxed_slice(),
            written: [0; LAYER_COUNT],
            depth: 0,
            open: Some(0),
        }
    }

    /// Append as much of `captured` as there is room for in the open layer, and
    /// report how many frames that was.
    ///
    /// Frames land in the open layer, and only there. An empty buffer has the
    /// take open, so a buffer that is only ever recorded into is a single
    /// layer; every later layer is opened by [`overdub`](Self::overdub), and
    /// [`undo`](Self::undo) leaves none open. Nothing is taken while none is,
    /// which is what keeps a take from growing past the loop the player heard
    /// when they undid a layer over it.
    ///
    /// A result below the length of `captured` means the layer is full and the
    /// rest were dropped: a take has reached the length the profile allows, or
    /// an overdub has reached the end of the loop it lies over. A full layer
    /// takes nothing and reports nothing taken, which is the same outcome one
    /// block later — it never grows to fit the rest, and never panics for being
    /// asked.
    pub fn record(&mut self, captured: &[f32]) -> usize {
        let Some(open) = self.open else {
            return 0;
        };

        let taken = captured.len().min(self.vacant());
        let at = open * self.capacity() + self.written[open];
        self.layers[at..at + taken].copy_from_slice(&captured[..taken]);
        self.written[open] += taken;
        self.depth = self.depth.max(open + 1);

        taken
    }

    /// Open a layer over the loop, and report whether there was one to open.
    ///
    /// A refusal means the stack is [`LAYERS`](Self::LAYERS) deep, or there is
    /// no loop yet to lie over — an overdub of nothing would be a layer the
    /// loop's length holds at no frames, which takes every block a player gives
    /// it and keeps none. What was recorded is left alone and the layer that
    /// was open stays open, so a caller that ignores the answer keeps recording
    /// where it was rather than losing the block.
    ///
    /// ```
    /// use motif::device::DeviceProfile;
    /// use motif::looper::LoopBuffer;
    ///
    /// let mut captured = LoopBuffer::for_profile(DeviceProfile::TARGET.audio);
    /// captured.record(&[0.25, 0.5]);
    ///
    /// captured.overdub();
    /// captured.record(&[0.125, 0.125]);
    ///
    /// let mut heard = [0.0; 2];
    /// captured.mix_into(&mut heard, 0);
    ///
    /// assert_eq!(heard, [0.375, 0.625]);
    /// ```
    pub fn overdub(&mut self) -> bool {
        if self.depth == Self::LAYERS || self.is_empty() {
            return false;
        }

        self.written[self.depth] = 0;
        self.open = Some(self.depth);
        self.depth += 1;

        true
    }

    /// Take the most recent overdub off the loop, and report whether there was
    /// one to take.
    ///
    /// The take is not undone. It is the loop rather than a layer over it, and
    /// the point of undo is to lose a mistake while the loop plays on, so
    /// emptying a loop is [`clear`](Self::clear)'s to do and nothing else's.
    ///
    /// No layer is left open, so the blocks still arriving from a player who
    /// has not yet let go of the button are taken nowhere rather than appended
    /// to the take. Recording again means opening a layer again.
    ///
    /// The samples of an undone layer are not overwritten, only its length is
    /// dropped to nothing. Nothing reads past a layer's length, so they are
    /// unreachable rather than stale, and undo is a couple of stores wherever
    /// it is called from.
    pub fn undo(&mut self) -> bool {
        if self.depth < 2 {
            return false;
        }

        self.depth -= 1;
        self.written[self.depth] = 0;
        self.open = None;

        true
    }

    /// Empty the loop, back to what [`for_profile`](Self::for_profile) returned.
    ///
    /// The storage is kept, so this is safe to call from the audio callback and
    /// the buffer is ready to take a new loop straight away.
    pub fn clear(&mut self) {
        self.written = [0; LAYER_COUNT];
        self.depth = 0;
        self.open = Some(0);
    }

    /// Add the loop, from frame `from`, into `block`, and report how many
    /// frames that was.
    ///
    /// Every layer is summed into what `block` already holds, so a caller
    /// mixing the loop over live input passes the block it rendered and gets
    /// both. It follows that a caller wanting the loop alone passes silence:
    /// zeroing here would make the loop the only thing that could be heard.
    ///
    /// Fewer frames than `block` holds means the loop ended inside it, and the
    /// rest of `block` is left as it was — the loop does not repeat here, and a
    /// caller wanting the frames after the end asks for them from the position
    /// they start at.
    pub fn mix_into(&self, block: &mut [f32], from: usize) -> usize {
        let wanted = block.len().min(self.len().saturating_sub(from));
        if wanted == 0 {
            return 0;
        }

        let block = &mut block[..wanted];

        for (layer, recorded) in self
            .layers
            .chunks_exact(self.capacity())
            .zip(self.written)
            .take(self.depth)
        {
            let heard = recorded.saturating_sub(from).min(wanted);
            for (mixed, sample) in block.iter_mut().zip(&layer[from..from + heard]) {
                *mixed += sample;
            }
        }

        wanted
    }

    /// How many frames long the loop is.
    pub fn len(&self) -> usize {
        self.written[0]
    }

    /// Whether nothing has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many layers hold audio, from none up to [`LAYERS`](Self::LAYERS).
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// How many frames the next [`record`](Self::record) can take.
    ///
    /// That is the room left in the open layer: what is left of the buffer
    /// while the take is open, what is left of the loop under an overdub, and
    /// nothing at all while no layer is open. It answers what the next block
    /// will do rather than how much of the buffer is spent, so a caller after
    /// the length of the loop wants [`len`](Self::len).
    pub fn vacant(&self) -> usize {
        let Some(open) = self.open else {
            return 0;
        };
        let room = if open == 0 {
            self.capacity()
        } else {
            self.len()
        };

        room - self.written[open]
    }

    /// The most frames a layer can ever hold.
    pub fn capacity(&self) -> usize {
        self.layers.len() / Self::LAYERS
    }
}
