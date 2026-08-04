//! The passage of time, as the event loop is allowed to see it.
//!
//! A loop that keeps a frame budget has to read a clock and wait on one, and
//! both are the kind of thing a test cannot assert on without becoming slow and
//! flaky. Behind a trait, a frame that took 3 ms is something a test states
//! rather than something it spends.
//!
//! ```
//! use std::time::Duration;
//!
//! use motif::ui::{Clock, ScriptedClock};
//!
//! let mut clock = ScriptedClock::new([Duration::ZERO, Duration::from_millis(4)]);
//!
//! let started = clock.now();
//! let ended = clock.now();
//! clock.sleep(Duration::from_millis(29));
//!
//! assert_eq!(ended - started, Duration::from_millis(4));
//! assert_eq!(clock.slept(), [Duration::from_millis(29)]);
//! ```

use std::thread;
use std::time::{Duration, Instant};

/// A clock that can be read and waited on.
pub trait Clock {
    /// What the time is now.
    fn now(&mut self) -> Instant;

    /// Give up the rest of `duration`.
    fn sleep(&mut self, duration: Duration);
}

/// The clock the machine keeps.
#[derive(Debug, Default)]
pub struct SystemClock;

impl SystemClock {
    /// A clock reading the machine's monotonic time.
    pub fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

/// A clock with no machine behind it, reading times given in advance.
///
/// It exists so that a test can say how long each frame took and read back what
/// the loop did about it. Sleeping records the request and returns at once: the
/// point of scripting time is not to pass any.
///
/// Readings are offsets from when the clock was made, so a script is written as
/// the elapsed times a loop would observe. A clock that has read its whole
/// script keeps returning the last reading rather than running off the end,
/// which is the reading a loop asking one more time should see.
#[derive(Debug, Default)]
pub struct ScriptedClock {
    started: Option<Instant>,
    readings: Vec<Duration>,
    read: usize,
    slept: Vec<Duration>,
}

impl ScriptedClock {
    /// A clock that will read back `readings`, in order.
    pub fn new(readings: impl IntoIterator<Item = Duration>) -> Self {
        Self {
            started: None,
            readings: readings.into_iter().collect(),
            read: 0,
            slept: Vec::new(),
        }
    }

    /// Every duration the clock was asked to sleep, in order.
    pub fn slept(&self) -> &[Duration] {
        &self.slept
    }

    fn reading(&self) -> Duration {
        let last = self.readings.len().saturating_sub(1);

        self.readings
            .get(self.read.min(last))
            .copied()
            .unwrap_or_default()
    }
}

impl Clock for ScriptedClock {
    fn now(&mut self) -> Instant {
        let started = *self.started.get_or_insert_with(Instant::now);
        let reading = self.reading();
        self.read += 1;

        started + reading
    }

    fn sleep(&mut self, duration: Duration) {
        self.slept.push(duration);
    }
}
