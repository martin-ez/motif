//! What the player is doing with the loop, and where the loop is kept.
//!
//! [`Transport`] is the state a player drives, [`LoopBuffer`] holds the samples
//! it governs, [`LoopEngine`] owns both on the audio thread, [`take_handoff`]
//! carries a finished take off it, and [`analysing`] is the thread it reaches.
//!
//! [`LoopBuffer`] is sized from the device profile and allocated in setup, so
//! the longest loop is a constraint the machine states rather than an accident
//! of free memory: one grown to meet a long take would allocate on the thread
//! that may not. Layers are a fixed stack of [`LoopBuffer::LAYERS`] for the
//! same reason, and are summed as the loop is read: a running mix would leave
//! [`LoopBuffer::undo`] subtracting a layer a float sum cannot restore exactly.
//!
//! One sample per frame, as the ring across the audio boundary carries them: a
//! channel layout belongs to a device, not to this side of the callback.

use std::ops::Range;

use crate::audio::held;
use crate::device::AudioProfile;

mod analyst;
mod engine;
mod marks;
mod page;
mod position;
mod take;
mod transport;
mod waveform;

pub use analyst::{analyse, analysing};
pub use engine::LoopEngine;
pub use marks::{LoopMarks, Mark, MarksReader, MarksWriter, marks_handoff};
pub use page::LooperPage;
pub use position::{LoopPosition, PositionReader, PositionWriter, position_meter};
pub use take::{FinishedTake, TakeReader, TakeWriter, take_handoff};
pub use transport::Transport;
pub use waveform::{Extremes, LoopWaveform, WaveformReader, WaveformWriter, waveform_meter};

const LAYER_COUNT: usize = 8;

/// The samples of a loop, in storage that is allocated once and never grows.
///
/// A loop is the sum of its layers, taken as it is read and [`held`] inside
/// full scale. Nothing under the ceiling moves as the stack deepens: what a
/// layer holds is what was played into it, so an overdub never quietens the
/// take and [`undo`](Self::undo) gives back exactly what was there.
pub struct LoopBuffer {
    layers: Box<[f32]>,
    written: [usize; LAYER_COUNT],
    cursor: [usize; LAYER_COUNT],
    depth: usize,
    open: Option<usize>,
    waveform: LoopWaveform,
    stale: Option<usize>,
}

impl LoopBuffer {
    /// How many layers a loop is built from: the take, and seven overdubs over
    /// it.
    ///
    /// A layer is a buffer of the profile's longest loop, so depth is bought
    /// with memory: at
    /// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET), 6.1 MB
    /// a layer and 49 MB for the stack.
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
            cursor: [0; LAYER_COUNT],
            depth: 0,
            open: Some(0),
            waveform: LoopWaveform::EMPTY,
            stale: None,
        }
    }

    /// Record `captured` into the open layer, and report how many frames that
    /// was. It never grows the buffer, and never panics.
    ///
    /// Frames land in the open layer and nowhere else: an empty buffer has the
    /// take open, [`overdub`](Self::overdub) opens each later layer, and
    /// [`undo`](Self::undo) leaves none open.
    ///
    /// A result short of `captured` means the take filled the buffer and the
    /// rest were dropped. A layer carries on from where the last block left it,
    /// round the boundary and over its own first pass, so it takes every frame.
    pub fn record(&mut self, captured: &[f32]) -> usize {
        match self.open {
            None => 0,
            Some(0) => self.append(captured),
            Some(open) => self.lay_round(open, captured),
        }
    }

    fn append(&mut self, captured: &[f32]) -> usize {
        let taken = captured.len().min(self.vacant());
        let from = self.cursor[0];
        self.layers[from..from + taken].copy_from_slice(&captured[..taken]);
        self.written[0] += taken;
        self.cursor[0] += taken;
        self.depth = self.depth.max(1);
        self.summarise(from, taken);

        taken
    }

    fn lay_round(&mut self, open: usize, captured: &[f32]) -> usize {
        let to_the_boundary = (self.len() - self.cursor[open]).min(captured.len());
        let (before, after) = captured.split_at(to_the_boundary);

        self.lay(open, before);
        for repeat in after.chunks(self.len()) {
            self.lay(open, repeat);
        }

        captured.len()
    }

    fn lay(&mut self, open: usize, captured: &[f32]) {
        let (len, from) = (self.len(), self.cursor[open]);
        let at = open * self.capacity() + from;
        self.layers[at..at + captured.len()].copy_from_slice(captured);
        self.cursor[open] = (from + captured.len()) % len;
        self.written[open] = (self.written[open] + captured.len()).min(len);
        self.summarise(from, captured.len());
    }

    fn summarise(&mut self, from: usize, taken: usize) {
        let Self {
            layers,
            written,
            cursor,
            depth,
            waveform,
            ..
        } = self;
        let sum = Sum {
            layers,
            capacity: layers.len() / LAYER_COUNT,
            cursor: *cursor,
            written: *written,
            depth: *depth,
            len: written[0],
        };

        waveform.take(from, (0..taken).map(|offset| held(sum.at(from + offset))));
    }

    /// Open a layer over the loop at frame `at`, and report whether there was
    /// one to open.
    ///
    /// `at` is where the player punched in, so what arrives next is heard where
    /// it was played; a position at or past the end wraps into the loop. Refused
    /// when the stack is [`LAYERS`](Self::LAYERS) deep, or when there is no loop
    /// yet to lie over: a layer bounded by a loop of no frames would take
    /// nothing, ever. Either way the open layer stays open, so a caller that
    /// ignores the answer keeps recording where it was.
    ///
    /// ```
    /// use motif::device::DeviceProfile;
    /// use motif::looper::LoopBuffer;
    ///
    /// let mut captured = LoopBuffer::for_profile(DeviceProfile::TARGET.audio);
    /// captured.record(&[0.25, 0.5]);
    ///
    /// captured.overdub(1);
    /// captured.record(&[0.125]);
    ///
    /// let mut heard = [0.0; 2];
    /// captured.mix_into(&mut heard, 0);
    ///
    /// assert_eq!(heard, [0.25, 0.625]);
    /// ```
    pub fn overdub(&mut self, at: usize) -> bool {
        if self.depth == Self::LAYERS || self.is_empty() {
            return false;
        }

        self.written[self.depth] = 0;
        self.cursor[self.depth] = at % self.len();
        self.open = Some(self.depth);
        self.depth += 1;

        true
    }

    /// Take the most recent overdub off the loop, and report whether there was
    /// one to take.
    ///
    /// The take is not undone: it is the loop rather than a layer over it, so
    /// emptying a loop is [`clear`](Self::clear)'s alone. No layer is left open
    /// afterwards, so blocks still arriving from a player who has not let go of
    /// the button are taken nowhere.
    ///
    /// A couple of stores wherever it is called from: the summary is left to
    /// [`resummarise`](Self::resummarise).
    pub fn undo(&mut self) -> bool {
        if self.depth < 2 {
            return false;
        }

        self.depth -= 1;
        self.written[self.depth] = 0;
        self.cursor[self.depth] = 0;
        self.open = None;
        self.stale = Some(0);

        true
    }

    /// Repaint up to `frames` of the summary from the layers that remain, and
    /// report whether any of the loop is still to cover.
    ///
    /// [`undo`](Self::undo) drops a layer without sweeping the loop again, so
    /// the summary keeps showing it until a cursor walking the loop reaches
    /// each bucket. Stepping that cursor a block at a time is what keeps the
    /// caller bounded, which is what the audio thread needs; a loop with
    /// nothing undone has nothing to cover and answers `false`.
    pub fn resummarise(&mut self, frames: usize) -> bool {
        let Some(from) = self.stale else {
            return false;
        };

        let taken = frames.min(self.len() - from);
        self.summarise(from, taken);
        let covered = from + taken;
        self.stale = (covered < self.len()).then_some(covered);

        self.stale.is_some()
    }

    /// Empty the loop, back to what [`for_profile`](Self::for_profile) returned.
    ///
    /// The storage is kept, so this is safe on the audio callback and the
    /// buffer takes a new loop straight away.
    pub fn clear(&mut self) {
        self.written = [0; LAYER_COUNT];
        self.cursor = [0; LAYER_COUNT];
        self.depth = 0;
        self.open = Some(0);
        self.waveform = LoopWaveform::EMPTY;
        self.stale = None;
    }

    /// The shape of the loop, as the thread that draws it is given it.
    ///
    /// Kept as the loop is recorded rather than measured on demand: summarising
    /// the whole of a long loop is a pass the callback has no room for, and
    /// folding each block in as it arrives is one it already makes. It is the
    /// held sum rather than the raw one, so what is drawn is what is heard. An
    /// undone layer is swept out by [`resummarise`](Self::resummarise), so this
    /// trails an undo by up to a lap of the loop.
    pub const fn waveform(&self) -> &LoopWaveform {
        &self.waveform
    }

    /// Add the loop, from frame `from`, into `block`, and report how many
    /// frames that was.
    ///
    /// Layers are summed and held inside full scale, then added to what `block`
    /// holds: a caller mixing the loop over live input passes the block it
    /// rendered, and the ceiling is on the loop rather than on that block.
    ///
    /// A result below the length of `block` means the loop ended inside it,
    /// leaving the rest as it was. The loop does not repeat here; that is
    /// [`play_into`](Self::play_into).
    pub fn mix_into(&self, block: &mut [f32], from: usize) -> usize {
        let wanted = block.len().min(self.len().saturating_sub(from));
        self.add_held(&mut block[..wanted], from);

        wanted
    }

    /// Play the loop into the whole of `block`, from frame `from`, and report
    /// the frame it left the playhead on.
    ///
    /// A boundary inside `block` is crossed there, so a loop of any length
    /// repeats without drift or a seam, and a short one as often as it fits.
    ///
    /// Layers are summed and held inside full scale, then added to what `block`
    /// holds; an empty loop is left alone. A `from` at or past the end restarts
    /// the loop, so a playhead kept across a change of length cannot hold a
    /// phase of its own.
    ///
    /// ```
    /// use motif::device::DeviceProfile;
    /// use motif::looper::LoopBuffer;
    ///
    /// let mut captured = LoopBuffer::for_profile(DeviceProfile::TARGET.audio);
    /// captured.record(&[0.25, 0.5, 0.75]);
    ///
    /// let mut heard = [0.0; 5];
    /// let playhead = captured.play_into(&mut heard, 0);
    ///
    /// assert_eq!(heard, [0.25, 0.5, 0.75, 0.25, 0.5]);
    /// assert_eq!(playhead, 2);
    /// ```
    pub fn play_into(&self, block: &mut [f32], from: usize) -> usize {
        if self.is_empty() {
            return 0;
        }

        let playhead = if from < self.len() { from } else { 0 };
        let to_the_boundary = (self.len() - playhead).min(block.len());
        let (before, after) = block.split_at_mut(to_the_boundary);

        self.add_held(before, playhead);
        for repeat in after.chunks_mut(self.len()) {
            self.add_held(repeat, 0);
        }

        (playhead + block.len()) % self.len()
    }

    fn add_held(&self, block: &mut [f32], from: usize) {
        let sum = self.sum();

        for (offset, mixed) in block.iter_mut().enumerate() {
            *mixed += held(sum.at(from + offset));
        }
    }

    fn sum(&self) -> Sum<'_> {
        Sum {
            layers: &self.layers,
            capacity: self.capacity(),
            cursor: self.cursor,
            written: self.written,
            depth: self.depth,
            len: self.len(),
        }
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

    /// How much of the open layer is still to be covered.
    ///
    /// The rest of the buffer under the take, which is what the next
    /// [`record`](Self::record) can take before it starts dropping frames; the
    /// part of the loop an overdub has yet to reach, which bounds nothing,
    /// because a layer laps rather than filling; and nothing at all while no
    /// layer is open. For the length of the loop, use [`len`](Self::len).
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

fn spans(cursor: usize, recorded: usize, len: usize) -> [Range<usize>; 2] {
    let before_the_cursor = cursor.min(recorded);
    let past_the_boundary = recorded - before_the_cursor;

    [
        cursor - before_the_cursor..cursor,
        len - past_the_boundary..len,
    ]
}

struct Sum<'a> {
    layers: &'a [f32],
    capacity: usize,
    cursor: [usize; LAYER_COUNT],
    written: [usize; LAYER_COUNT],
    depth: usize,
    len: usize,
}

impl Sum<'_> {
    fn at(&self, frame: usize) -> f32 {
        self.layers
            .chunks_exact(self.capacity)
            .zip(self.cursor.into_iter().zip(self.written))
            .take(self.depth)
            .filter(|&(_, (cursor, recorded))| {
                spans(cursor, recorded, self.len)
                    .iter()
                    .any(|span| span.contains(&frame))
            })
            .map(|(layer, _)| layer[frame])
            .sum()
    }
}
