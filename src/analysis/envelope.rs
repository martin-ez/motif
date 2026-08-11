//! Where a take got louder, which is the only thing a beat tracker reads.
//!
//! Nothing here transforms anything: the strength of an onset is a difference
//! between two short-time energies, computed in the time domain, so the front
//! end costs one pass over the samples and carries no dependency.

use std::time::Duration;

const COMPRESSION: f32 = 10_000.0;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Where a take got louder, frame by frame.
///
/// The half-wave-rectified first difference of a log-compressed short-time
/// energy: a sound that rises reads as an onset, and one that merely goes on
/// sounding does not. Compression is by `ln(1 + 10000 e)`, which is what keeps
/// a click played softly visible beside one played hard — the range between
/// them is a factor of hundreds, and a difference taken on the raw energy sees
/// only the loud one.
///
/// ```
/// use motif::analysis::Envelope;
/// use std::time::Duration;
///
/// let struck = (0..8_000).map(|frame| if (100..200).contains(&frame) { 0.5 } else { 0.0 });
/// let envelope = Envelope::of(struck, 8_000);
///
/// assert!(envelope.at(Duration::from_millis(12)) > 0.0);
/// assert_eq!(envelope.at(Duration::from_millis(500)), 0.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    hop: Duration,
    strength: Vec<f32>,
}

impl Envelope {
    /// How often the envelope reads the audio.
    ///
    /// 4 ms, which is a whole number of frames at every rate the target
    /// profile opens a device at, and places an onset an order of magnitude
    /// inside the 70 ms window a beat is scored against.
    pub const HOP: Duration = Duration::from_millis(4);

    /// Take the envelope of `samples`, timed against a clock of `sample_rate`
    /// frames per second.
    ///
    /// The samples arrive as an iterator because a finished take is read off
    /// the loop one frame at a time, and the envelope is hundreds of times
    /// smaller than the audio it is taken over.
    pub fn of(samples: impl IntoIterator<Item = f32>, sample_rate: u32) -> Self {
        let per_hop = frames_per_hop(sample_rate);
        let mut strength = Vec::new();
        let mut quiet = 0.0;
        let mut energy = 0.0;
        let mut counted = 0;

        for sample in samples {
            energy += sample * sample;
            counted += 1;
            if counted == per_hop {
                quiet = risen(&mut strength, energy / counted as f32, quiet);
                energy = 0.0;
                counted = 0;
            }
        }
        if counted > 0 {
            risen(&mut strength, energy / counted as f32, quiet);
        }

        Self {
            hop: hop_of(per_hop, sample_rate),
            strength,
        }
    }

    /// How much the take rose at each hop, in the order the hops fall.
    pub fn strength(&self) -> &[f32] {
        &self.strength
    }

    /// How long one hop is, which is [`HOP`](Self::HOP) rounded to a whole
    /// number of frames of the clock the samples were timed against.
    pub fn hop(&self) -> Duration {
        self.hop
    }

    /// How much of the take the envelope covers.
    pub fn span(&self) -> Duration {
        self.hop * self.strength.len() as u32
    }

    /// How much the take rose at `when`, and nothing past the end of it.
    ///
    /// A moment inside a hop reads that hop, so this is the strength of the
    /// frame covering `when` rather than an interpolation between two.
    pub fn at(&self, when: Duration) -> f32 {
        self.strength
            .get(self.frame(when))
            .copied()
            .unwrap_or_default()
    }

    fn frame(&self, when: Duration) -> usize {
        (when.as_nanos() / self.hop.as_nanos()) as usize
    }
}

fn risen(strength: &mut Vec<f32>, energy: f32, quiet: f32) -> f32 {
    let level = (1.0 + COMPRESSION * energy).ln();
    strength.push((level - quiet).max(0.0));

    level
}

fn frames_per_hop(sample_rate: u32) -> usize {
    let per_hop = Envelope::HOP.as_secs_f64() * f64::from(sample_rate);

    (per_hop.round() as usize).max(1)
}

fn hop_of(per_hop: usize, sample_rate: u32) -> Duration {
    let nanos = per_hop as u64 * NANOS_PER_SECOND / u64::from(sample_rate);

    Duration::from_nanos(nanos.max(1))
}
