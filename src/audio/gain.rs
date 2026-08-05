//! A level the player moves, and the ramp that keeps moving it from clicking.
//!
//! The first user-controlled parameter in the crate, so the shape here is the
//! one every parameter after it should take: the value the player asked for is
//! kept apart from the value the audio is being multiplied by, and the second
//! walks towards the first a frame at a time.
//!
//! A linear ramp rather than a filter approaching the target. It arrives
//! exactly and in a stated time, where a one-pole is asymptotic and leaves a
//! tail of denormals on the thread that can least afford them.

const RAMP_MILLISECONDS: usize = 10;
const MILLISECONDS_PER_SECOND: usize = 1_000;
const UNITY: f32 = 1.0;
const SILENCE: f32 = 0.0;

fn ramp_frames(sample_rate: u32) -> usize {
    (sample_rate as usize * RAMP_MILLISECONDS / MILLISECONDS_PER_SECOND).max(1)
}

/// A gain that moves to what it was set to rather than jumping there.
///
/// Mute is not a second multiplier but a target of its own: it makes the gain
/// being reached for zero while keeping the one that was asked for, so muting
/// and unmuting ramp exactly as a change of level does, and a level set while
/// muted is what unmuting arrives at.
///
/// Every change takes the same time, however far it goes, because the step is
/// the distance divided by the ramp.
///
/// ```
/// use motif::audio::Gain;
///
/// let mut gain = Gain::unity();
/// gain.prepare(48_000);
/// gain.set_muted(true);
///
/// let mut block = [1.0; 8];
/// gain.apply(&mut block);
///
/// assert!(block[7] < block[0]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gain {
    target: f32,
    muted: bool,
    current: f32,
    step: f32,
    frames: usize,
    remaining: usize,
}

impl Gain {
    /// How long a change takes to arrive, in milliseconds.
    ///
    /// Long enough that a step across the whole range is inaudible, short
    /// enough that a hand on the encoder feels the level move with it. A ramp
    /// is what stops a change clicking; how long it is trades one artefact for
    /// the other, and ten milliseconds is where both are below noticing.
    pub const RAMP: usize = RAMP_MILLISECONDS;

    /// A gain that passes what it is given, unmuted and already there.
    pub const fn unity() -> Self {
        Self {
            target: UNITY,
            muted: false,
            current: UNITY,
            step: SILENCE,
            frames: 1,
            remaining: 0,
        }
    }

    /// Work out the ramp for a device running at `sample_rate`.
    ///
    /// Called where a path is prepared, which is the thread that may allocate
    /// and the first place the granted rate is known. A gain that was never
    /// prepared arrives in a single frame, having no ramp to spread a change
    /// over.
    pub fn prepare(&mut self, sample_rate: u32) {
        self.frames = ramp_frames(sample_rate);
        self.retarget();
    }

    /// Head for `target`, a linear multiplier where `1.0` is unity.
    ///
    /// A target that is not a number is refused and the last one kept: it
    /// arrives from a queue that accepts any bit pattern as a gain, and one
    /// NaN multiplied into the block would silence the output for the rest of
    /// the run. Below silence is taken as silence, a gain control having no
    /// meaning under it.
    pub fn set_target(&mut self, target: f32) {
        if !target.is_finite() {
            return;
        }

        self.target = target.max(SILENCE);
        self.retarget();
    }

    /// Head for silence, or back to the target.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.retarget();
    }

    /// The gain that was asked for, whether or not it is muted.
    pub const fn target(&self) -> f32 {
        self.target
    }

    /// Whether the gain is heading for silence rather than its target.
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// Scale `block` in place, moving a step along the ramp per frame.
    ///
    /// Runs on the audio thread: one multiply and one add per frame, no
    /// allocation, and no branch that can panic.
    pub fn apply(&mut self, block: &mut [f32]) {
        let reaching = self.reaching();

        for sample in block.iter_mut() {
            *sample *= self.current;
            self.advance(reaching);
        }
    }

    const fn reaching(&self) -> f32 {
        if self.muted { SILENCE } else { self.target }
    }

    fn retarget(&mut self) {
        self.step = (self.reaching() - self.current) / self.frames as f32;
        self.remaining = self.frames;
    }

    fn advance(&mut self, reaching: f32) {
        let Some(left) = self.remaining.checked_sub(1) else {
            return;
        };

        self.remaining = left;
        self.current = if left == 0 {
            reaching
        } else {
            self.current + self.step
        };
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::unity()
    }
}
