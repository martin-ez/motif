//! Carrying the news that a device faulted, from the callback that heard it to
//! the thread that can do something about it.
//!
//! A host reports device loss by calling a stream's error callback, which runs
//! under the same rules as the data callback: no allocation, no lock, no
//! panicking path. So the fault crosses as one atomic compare-exchange.
//!
//! The first fault wins, rather than the most recent. A device that goes away
//! reports again on every callback that follows, and each of those is a
//! consequence of the first; latching the first keeps the cause.
//!
//! This is the one crossing with no single writer — a duplex stream has two
//! error callbacks, and either may notice — so reporting takes `&self` where
//! publishing a level takes `&mut self`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::DeviceError;

/// Build a fault latch, and split it into the reporting end and the reading
/// end.
///
/// Allocates here and never again, so this belongs in setup, before the stream
/// starts.
///
/// ```
/// use motif::audio::{DeviceError, fault_channel};
///
/// let (reporter, reader) = fault_channel();
///
/// assert_eq!(reader.read(), None);
///
/// reporter.report(DeviceError::DeviceNotAvailable);
///
/// assert_eq!(reader.read(), Some(DeviceError::DeviceNotAvailable));
/// ```
pub fn fault_channel() -> (FaultReporter, FaultReader) {
    let latched = Arc::new(AtomicU8::new(NO_FAULT));

    (
        FaultReporter {
            latched: Arc::clone(&latched),
        },
        FaultReader { latched },
    )
}

/// The reporting end of a fault latch, held by whichever thread hears the
/// device fail.
///
/// This is the end a stream's error callback holds. Clone it to give both
/// callbacks of a duplex stream a way in; cloning allocates, so clone at setup.
#[derive(Clone)]
pub struct FaultReporter {
    latched: Arc<AtomicU8>,
}

impl FaultReporter {
    /// Latch `error` as the reason the device failed, if nothing has been
    /// latched yet.
    ///
    /// One compare-exchange and no loop, so this is safe to call from an error
    /// callback. A second report is dropped rather than replacing the first:
    /// see the module documentation for why the cause beats the consequence.
    pub fn report(&self, error: DeviceError) {
        let _ = self.latched.compare_exchange(
            NO_FAULT,
            encoded(error),
            Ordering::Release,
            Ordering::Relaxed,
        );
    }
}

/// The reading end of a fault latch, held by whichever thread can rebuild the
/// stream.
pub struct FaultReader {
    latched: Arc<AtomicU8>,
}

impl FaultReader {
    /// Why the device failed, or `None` while it has not.
    ///
    /// Clears nothing. A latch that has caught a fault reports it forever, so
    /// recovery means replacing the stream rather than resetting this.
    pub fn read(&self) -> Option<DeviceError> {
        decoded(self.latched.load(Ordering::Acquire))
    }
}

const NO_FAULT: u8 = 0;

fn encoded(error: DeviceError) -> u8 {
    match error {
        DeviceError::NoInputDevice => 1,
        DeviceError::NoOutputDevice => 2,
        DeviceError::UnsupportedConfig => 3,
        DeviceError::DeviceNotAvailable => 4,
        DeviceError::PermissionDenied => 5,
        DeviceError::BackendFailure => 6,
    }
}

fn decoded(code: u8) -> Option<DeviceError> {
    match code {
        1 => Some(DeviceError::NoInputDevice),
        2 => Some(DeviceError::NoOutputDevice),
        3 => Some(DeviceError::UnsupportedConfig),
        4 => Some(DeviceError::DeviceNotAvailable),
        5 => Some(DeviceError::PermissionDenied),
        6 => Some(DeviceError::BackendFailure),
        _ => None,
    }
}
