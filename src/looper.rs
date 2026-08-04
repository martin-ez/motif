//! What the player is doing with the loop, and where the loop is kept.
//!
//! [`Transport`] is the state a player drives with record, play and stop;
//! [`LoopBuffer`] holds the samples those states govern.
//!
//! [`LoopBuffer`] is sized from the device profile and allocated before the
//! stream starts, so the longest loop a player can capture is a constraint the
//! machine states rather than an accident of how much memory happens to be
//! free. On hardware it is the former either way, and a buffer that grew to
//! meet a long take would have to allocate on the thread that may not.
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

/// The samples of a loop, in storage that is allocated once and never grows.
pub struct LoopBuffer {
    frames: Box<[f32]>,
    recorded: usize,
}

impl LoopBuffer {
    /// Allocate a buffer for the longest loop `profile` allows.
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
    /// captured.record(&[0.1, 0.2]);
    ///
    /// assert_eq!(captured.frames(), &[0.1, 0.2]);
    /// assert_eq!(captured.capacity(), profile.max_loop_frames());
    /// ```
    pub fn for_profile(profile: AudioProfile) -> Self {
        let capacity = profile.max_loop_frames();
        assert!(capacity > 0, "a loop buffer holds no loop without frames");

        Self {
            frames: vec![0.0; capacity].into_boxed_slice(),
            recorded: 0,
        }
    }

    /// Append as much of `captured` as there is room for, and report how many
    /// frames that was.
    ///
    /// A result below the length of `captured` means the buffer is full and the
    /// rest were dropped: the take has reached the length the profile allows,
    /// and the caller decides what that means. A full buffer takes nothing and
    /// reports nothing taken, which is the same outcome one block later — it
    /// never grows to fit the rest, and never panics for being asked.
    pub fn record(&mut self, captured: &[f32]) -> usize {
        let taken = captured.len().min(self.vacant());
        self.frames[self.recorded..self.recorded + taken].copy_from_slice(&captured[..taken]);
        self.recorded += taken;

        taken
    }

    /// The loop as it stands: every frame recorded so far, in order.
    pub fn frames(&self) -> &[f32] {
        &self.frames[..self.recorded]
    }

    /// How many frames have been recorded.
    pub fn len(&self) -> usize {
        self.recorded
    }

    /// Whether nothing has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.recorded == 0
    }

    /// How many frames can be recorded before the buffer is full.
    pub fn vacant(&self) -> usize {
        self.capacity() - self.recorded
    }

    /// The most frames the buffer can ever hold.
    pub fn capacity(&self) -> usize {
        self.frames.len()
    }
}
