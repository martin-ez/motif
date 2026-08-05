//! Putting the audio callback on a core of its own, and reporting what the
//! host allowed.
//!
//! The pinning is done to the thread that opens the stream, around the build
//! that spawns the callback's: a thread inherits the affinity mask of the one
//! that created it, so the callback is placed before it runs a block and no
//! syscall is made on it. The opening thread's own mask is put back afterwards.
//!
//! The scheduling class is not asked for here. `cpal` promotes its own worker
//! where the device is one that blocks rather than spins, which is a judgement
//! this crate cannot make from outside it, and macOS runs a CoreAudio device
//! thread under a time-constraint policy already. Both are [`Grant::Hosted`],
//! and a refusal comes back through the stream's error callback.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::device::DeviceProfile;

mod host;

/// Where the audio callback asks to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// The core it runs on, counted from zero.
    pub core: usize,
}

impl Placement {
    /// The last of `cores`, leaving the rest to analysis and the screen.
    ///
    /// Both callbacks of a duplex stream land on this one core. A profile
    /// reserves a core for audio rather than one per direction, and a capture
    /// and a render that each use a fraction of a block period fit in one.
    ///
    /// ```
    /// use motif::audio::Placement;
    /// use motif::device::DeviceProfile;
    ///
    /// let reserved = Placement::reserving_last_of(DeviceProfile::TARGET.cores);
    ///
    /// assert_eq!(reserved.core, 3);
    /// ```
    pub const fn reserving_last_of(cores: usize) -> Self {
        Self {
            core: cores.saturating_sub(1),
        }
    }

    /// The last core this process is actually allowed to run on, or the one
    /// [`DeviceProfile::TARGET`] describes where the host will not say.
    ///
    /// The running host rather than the profile, because a core is a physical
    /// thing: naming the target's fourth core on a machine that has two is a
    /// placement every host refuses, and one inside a restricted set is a
    /// placement this process may not make.
    pub fn available() -> Self {
        match host::owned_cores().last() {
            Some(&core) => Self { core },
            None => Self::reserving_last_of(DeviceProfile::TARGET.cores),
        }
    }
}

/// What a host did with one half of a [`Placement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// Nothing has been asked of the host.
    Unasked,
    /// The host did what was asked.
    Given,
    /// The host has a way to do it and would not, as a Linux without
    /// `CAP_SYS_NICE` refuses a real-time class.
    Refused,
    /// The host offers no way to do it, or none for the core that was named.
    Unavailable,
    /// The layer below does it, so nothing was asked.
    ///
    /// macOS runs a CoreAudio device thread under a time-constraint policy,
    /// and `cpal` promotes its own worker for a device that blocks rather than
    /// spins. Which of those happened is theirs to decide, not ours to report.
    Hosted,
}

impl Grant {
    /// What a host refusing with `errno` meant.
    ///
    /// A permission answer is a refusal — the call exists and was denied.
    /// Anything else is a host that has no way to do what was asked, which is
    /// what `EINVAL` says about a core the machine does not have.
    ///
    /// ```
    /// use motif::audio::Grant;
    ///
    /// assert_eq!(Grant::refusing(libc::EPERM), Grant::Refused);
    /// assert_eq!(Grant::refusing(libc::EINVAL), Grant::Unavailable);
    /// ```
    pub fn refusing(errno: i32) -> Self {
        match errno {
            libc::EPERM | libc::EACCES => Self::Refused,
            _ => Self::Unavailable,
        }
    }
}

/// What a host did with a whole [`Placement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// What became of the core that was asked for.
    pub affinity: Grant,
    /// What became of the callback's scheduling class.
    pub priority: Grant,
}

impl Placed {
    /// Nothing asked of the host, which is what a stream with no callback
    /// thread reports.
    pub const UNASKED: Self = Self {
        affinity: Grant::Unasked,
        priority: Grant::Unasked,
    };
}

/// What the layer below the placement does about the callback's scheduling
/// class, before anything reports otherwise.
pub const HOSTED_PRIORITY: Grant = if cfg!(any(target_os = "linux", target_os = "macos")) {
    Grant::Hosted
} else {
    Grant::Unavailable
};

/// Pin the calling thread to `placement` for the length of `building`, and
/// report what the host did alongside whatever `building` returned.
///
/// Whatever `building` spawns inherits the placement, which is how a callback
/// thread is placed without a syscall being made on it. The calling thread's
/// own mask is restored afterwards, so an application thread is not left on the
/// core the callback was given.
pub fn pinning<T>(placement: Placement, building: impl FnOnce() -> T) -> (Grant, T) {
    let previous = host::owned_cores();
    let granted = pinned_to(placement.core);
    let built = building();

    let _ = host::pin_to_cores(&previous);

    (granted, built)
}

fn pinned_to(core: usize) -> Grant {
    match host::pin_to_cores(&[core]) {
        Ok(()) => Grant::Given,
        Err(errno) => Grant::refusing(errno),
    }
}

/// Build a latch for the scheduling class the layer below grants, and split it
/// into the reporting end and the reading end.
///
/// Reads [`HOSTED_PRIORITY`] until something reports a refusal, and
/// [`Grant::Refused`] forever after. Allocates here and never again, so this
/// belongs in setup; clone the reporter to give both callbacks a way in.
///
/// ```
/// use motif::audio::{Grant, priority_latch};
///
/// let (reporter, reader) = priority_latch();
///
/// reporter.denied();
///
/// assert_eq!(reader.read(), Grant::Refused);
/// ```
pub fn priority_latch() -> (PriorityReporter, PriorityReader) {
    let denied = Arc::new(AtomicBool::new(false));

    (
        PriorityReporter {
            denied: Arc::clone(&denied),
        },
        PriorityReader { denied },
    )
}

/// The reporting end of a priority latch, held by whichever thread hears the
/// refusal.
#[derive(Clone)]
pub struct PriorityReporter {
    denied: Arc<AtomicBool>,
}

impl PriorityReporter {
    /// Report that the layer below was refused the scheduling class it asked
    /// for.
    ///
    /// One atomic store and no loop, so an error callback may reach it.
    pub fn denied(&self) {
        self.denied.store(true, Ordering::Release);
    }
}

/// The reading end of a priority latch, held by whichever thread reports it.
pub struct PriorityReader {
    denied: Arc<AtomicBool>,
}

impl PriorityReader {
    /// The scheduling class the callback is running under.
    ///
    /// Clears nothing: a refusal holds for the life of the stream, since
    /// nothing asks again.
    pub fn read(&self) -> Grant {
        if self.denied.load(Ordering::Acquire) {
            return Grant::Refused;
        }
        HOSTED_PRIORITY
    }
}
