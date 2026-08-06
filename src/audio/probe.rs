//! Timing a click out of the output and back in at the input, so that
//! "zero-latency passthrough" is a number rather than a claim.
//!
//! [`LatencyProbe`] is an [`AudioPath`] that plays a single-frame click and
//! counts the frames until it hears one, which over a loopback cable is the
//! whole round trip: the boundary's slack, both converters, and whatever the
//! device buffers at each end. It publishes over one atomic store, as the
//! meters beside it do, and allocates nothing in the callback.
//!
//! A take is not started until the stream has settled, and a click that does
//! not come back inside [`LISTENING`] stops the probe rather than starting
//! another: a second click would collect the first one's late return inside its
//! own window and report it as a short round trip. So an unplugged loop, and a
//! slow one, measure nothing at all rather than something wrong.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{AudioPath, Command, StreamConfig};

/// Blocks of round trip a passthrough is allowed to cost.
///
/// One of them is the boundary's own slack, the least that keeps playback from
/// outrunning capture; the other four are a conventional double buffer on each
/// of capture and playback. Stated in blocks rather than milliseconds so that
/// it follows whatever block size a device grants — at
/// [`DeviceProfile::TARGET`](crate::device::DeviceProfile::TARGET)'s 48 kHz in
/// 256-frame blocks it is 26.7 ms.
pub const ROUND_TRIP_BUDGET_BLOCKS: u32 = 5;

/// The amplitude a probe clicks at.
///
/// Full scale, and one frame long: an impulse has the sharpest onset there is,
/// and the onset is the whole of the measurement. A converter's anti-alias
/// filtering smears it over a handful of samples and so reads a fraction of a
/// millisecond late, which is well inside what a buffer added somewhere would
/// move the figure by.
pub const CLICK_AMPLITUDE: f32 = 1.0;

/// The fraction of [`CLICK_AMPLITUDE`] a return must reach to be the click.
///
/// A fraction of what went out rather than an absolute level, so it holds
/// however much the loop attenuates by and needs no noise floor measured to be
/// defensible. At 0.2 it sits about 14 dB below the click.
pub const DETECTION_FRACTION: f32 = 0.2;

/// How long a probe plays silence before each click.
///
/// Long enough for the ring to rise past its slack and for the transient of a
/// stream starting to pass, and it spaces takes far enough apart that a reader
/// polling the meter sees every one of them.
pub const SETTLING: Duration = Duration::from_millis(250);

/// How long a probe waits for its click before it gives up on the loop.
///
/// A click that has not come back by then stops the probe rather than starting
/// another take: clicking again would collect this one's late return inside the
/// next one's window and report it as a short round trip. A loop slower than
/// this measures nothing, which is what a cable is for.
pub const LISTENING: Duration = Duration::from_millis(250);

/// How long a click took to come back, in frames.
///
/// Frames rather than a duration, because a duration means nothing apart from
/// the rate it was measured at: pair it with the rate the device granted, in
/// [`duration`](Self::duration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundTrip {
    /// Frames between the click going out and coming back in.
    pub frames: u32,
}

impl RoundTrip {
    /// The most a round trip may cost, on a stream granted `block_size`.
    ///
    /// [`ROUND_TRIP_BUDGET_BLOCKS`] of them.
    ///
    /// ```
    /// use motif::audio::RoundTrip;
    ///
    /// assert_eq!(RoundTrip::budget(256).frames, 1_280);
    /// ```
    pub const fn budget(block_size: u32) -> Self {
        Self {
            frames: block_size.saturating_mul(ROUND_TRIP_BUDGET_BLOCKS),
        }
    }

    /// How long this is, at `sample_rate`.
    ///
    /// A stream running at no rate at all took no time, rather than dividing by
    /// zero: a configuration that is wrong should measure something useless
    /// instead of panicking on the thread that reads it.
    ///
    /// ```
    /// use std::time::Duration;
    /// use motif::audio::RoundTrip;
    ///
    /// assert_eq!(RoundTrip { frames: 48 }.duration(48_000), Duration::from_millis(1));
    /// ```
    pub const fn duration(self, sample_rate: u32) -> Duration {
        if sample_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos(self.frames as u64 * NANOSECONDS_PER_SECOND / sample_rate as u64)
    }

    /// Whether this is inside `budget`, which is the reading that matters.
    ///
    /// ```
    /// use motif::audio::RoundTrip;
    ///
    /// assert!(RoundTrip { frames: 1_280 }.within(RoundTrip::budget(256)));
    /// assert!(!RoundTrip { frames: 1_281 }.within(RoundTrip::budget(256)));
    /// ```
    pub const fn within(self, budget: Self) -> bool {
        self.frames <= budget.frames
    }
}

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const MILLISECONDS_PER_SECOND: u64 = 1_000;
const DETECTION_THRESHOLD: f32 = CLICK_AMPLITUDE * DETECTION_FRACTION;

/// What a probe has measured so far.
///
/// The count travels with the reading so that a caller polling for takes can
/// tell a fresh one from the same one read twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    /// How many round trips have been timed since the probe started.
    pub takes: u32,
    /// The most recent of them.
    pub round_trip: RoundTrip,
}

impl Measurement {
    fn packed(self) -> u64 {
        u64::from(self.takes) << 32 | u64::from(self.round_trip.frames)
    }

    fn unpacked(packed: u64) -> Option<Self> {
        let takes = (packed >> 32) as u32;
        if takes == 0 {
            return None;
        }

        Some(Self {
            takes,
            round_trip: RoundTrip {
                frames: packed as u32,
            },
        })
    }
}

/// Build a probe, and split it into the end that clicks and the end that reads
/// what came back.
///
/// The storage is allocated here and never again, so this belongs in setup,
/// before the stream starts.
///
/// ```
/// let (_probe, measured) = motif::audio::latency_probe();
///
/// assert!(measured.read().is_none());
/// ```
pub fn latency_probe() -> (LatencyProbe, RoundTripReader) {
    let published = Arc::new(AtomicU64::new(0));

    (
        LatencyProbe {
            published: Arc::clone(&published),
            stage: Stage::Settling { until: 0 },
            settling: 0,
            listening: 0,
            rendered: 0,
            takes: 0,
        },
        RoundTripReader { published },
    )
}

/// The path that measures its own round trip.
///
/// Runs a take at a time: silence while the stream settles, one click, then
/// listening for it, and it stops for good at the first click that does not
/// come back. Both ends of the loop are the caller's to wire — over a cable
/// this measures the hardware, and over anything else it measures that.
///
/// It hears nothing of the block it clicked in, whose frames were captured
/// before the click was played, so a round trip shorter than one block is not
/// measurable. The boundary's slack makes one that short impossible.
pub struct LatencyProbe {
    published: Arc<AtomicU64>,
    stage: Stage,
    settling: u64,
    listening: u64,
    rendered: u64,
    takes: u32,
}

impl LatencyProbe {
    fn publish(&mut self, frames: u64) {
        self.takes = self.takes.saturating_add(1);

        let measurement = Measurement {
            takes: self.takes,
            round_trip: RoundTrip {
                frames: frames as u32,
            },
        };
        self.published
            .store(measurement.packed(), Ordering::Release);
    }
}

impl AudioPath for LatencyProbe {
    /// Sizes the settling and listening spans in the frames the device granted,
    /// so a take takes the same time at whatever rate it was opened at.
    fn prepare(&mut self, config: StreamConfig) {
        self.settling = frames_in(SETTLING, config.sample_rate);
        self.listening = frames_in(LISTENING, config.sample_rate);
        self.stage = Stage::Settling {
            until: self.rendered + self.settling,
        };
    }

    /// Bounded by the shorter of the two slices, as every path is, and
    /// allocating nothing: the click is one store and the search one pass.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        let frames = playing.len().min(captured.len());
        if frames == 0 {
            return;
        }

        let started = self.rendered;
        let ended = started + frames as u64;

        self.stage = match self.stage {
            Stage::Settling { until } if started >= until => {
                playing[0] = CLICK_AMPLITUDE;
                Stage::Listening {
                    emitted_at: started,
                    until: started + self.listening,
                }
            }
            Stage::Listening { emitted_at, until } => match first_crossing(&captured[..frames]) {
                Some(at) => {
                    self.publish(started + at as u64 - emitted_at);
                    Stage::Settling {
                        until: ended + self.settling,
                    }
                }
                None if ended >= until => Stage::Silent,
                None => Stage::Listening { emitted_at, until },
            },
            waiting => waiting,
        };

        self.rendered = ended;
    }

    /// Nothing to answer: what a probe plays is its own to decide, and there is
    /// no command that changes it.
    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

#[derive(Clone, Copy)]
enum Stage {
    Settling { until: u64 },
    Listening { emitted_at: u64, until: u64 },
    Silent,
}

fn frames_in(span: Duration, sample_rate: u32) -> u64 {
    span.as_millis() as u64 * u64::from(sample_rate) / MILLISECONDS_PER_SECOND
}

fn first_crossing(captured: &[f32]) -> Option<usize> {
    captured
        .iter()
        .position(|sample| sample.abs() >= DETECTION_THRESHOLD)
}

/// The reading end of a probe, held by whichever thread reports it.
pub struct RoundTripReader {
    published: Arc<AtomicU64>,
}

impl RoundTripReader {
    /// What the probe has measured, or nothing until it has measured anything.
    ///
    /// Whatever was published before is replaced rather than queued, as a meter
    /// does: the take count is what says a reading is a new one.
    pub fn read(&self) -> Option<Measurement> {
        Measurement::unpacked(self.published.load(Ordering::Acquire))
    }
}
