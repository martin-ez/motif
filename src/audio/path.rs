//! What a stream plays, and the one choice that used to be the only one.
//!
//! A stream captures frames and plays frames; between the two is a decision
//! only a caller can make. [`AudioPath`] is where that decision goes: built on
//! the thread that opens the stream, called on the thread that may not
//! allocate, and the way anything a player is meant to hear — a loop, a
//! metronome click, a monitored input — reaches the audio callback at all.
//!
//! [`Passthrough`] is what every stream once did and now merely may: play the
//! frames it captured.

use super::StreamConfig;

/// What a stream plays, block by block.
///
/// The two halves sit either side of invariant 2:
/// [`prepare`](Self::prepare) runs where allocating is allowed and
/// [`render`](Self::render) where it is not.
pub trait AudioPath: Send + 'static {
    /// Prepare to run at `config`, before any block arrives.
    ///
    /// Called once, on the thread that opened the stream, and the only place a
    /// path may allocate. The device has the last word on the rate and the
    /// block size, so a path sized from the request would be sized from a
    /// guess.
    fn prepare(&mut self, config: StreamConfig);

    /// Play into `playing`, given the `captured` frames that arrived with it.
    ///
    /// One sample per frame in both directions and the same count in each: a
    /// channel layout belongs to the device, not here. `playing` arrives
    /// silent, so a path with nothing to play may leave it alone.
    ///
    /// Runs on the audio thread, so it may not allocate, lock or panic. Neither
    /// slice is bounded by what [`prepare`](Self::prepare) was told, so bound
    /// the work by the slices that were handed over.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]);
}

/// The path that plays the frames it captured.
///
/// What a stream did before anything could say otherwise, and what a caller
/// chooses to go on monitoring the input.
pub struct Passthrough;

impl Passthrough {
    /// A path that plays what it hears.
    pub const fn new() -> Self {
        Self
    }
}

impl Default for Passthrough {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPath for Passthrough {
    /// Nothing to prepare: what it plays is what it was handed, at whatever
    /// rate and block size that arrives in.
    fn prepare(&mut self, _config: StreamConfig) {}

    /// Copies as many frames as both slices have, so a caller who hands over
    /// two of different lengths gets silence for the difference rather than a
    /// panic on the audio thread.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        let frames = playing.len().min(captured.len());
        playing[..frames].copy_from_slice(&captured[..frames]);
    }
}
