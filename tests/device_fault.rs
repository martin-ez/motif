//! Carrying the news that a device faulted from the callback that heard it to
//! the thread that can do something about it.

use motif::audio::{DeviceError, fault_channel};

#[test]
fn a_channel_nobody_has_reported_to_carries_no_fault() {
    let (_reporter, reader) = fault_channel();

    assert_eq!(reader.read(), None);
}

#[test]
fn a_reported_fault_reads_back() {
    let (reporter, reader) = fault_channel();

    reporter.report(DeviceError::DeviceNotAvailable);

    assert_eq!(reader.read(), Some(DeviceError::DeviceNotAvailable));
}

#[test]
fn the_first_fault_wins_over_a_later_one() {
    let (reporter, reader) = fault_channel();

    reporter.report(DeviceError::DeviceNotAvailable);
    reporter.report(DeviceError::BackendFailure);

    assert_eq!(reader.read(), Some(DeviceError::DeviceNotAvailable));
}

#[test]
fn two_reporters_share_one_channel() {
    let (input, reader) = fault_channel();
    let output = input.clone();

    output.report(DeviceError::UnsupportedConfig);

    assert_eq!(reader.read(), Some(DeviceError::UnsupportedConfig));

    input.report(DeviceError::PermissionDenied);

    assert_eq!(reader.read(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn every_device_error_survives_the_crossing() {
    let errors = [
        DeviceError::NoSuchHost,
        DeviceError::NoInputDevice,
        DeviceError::NoOutputDevice,
        DeviceError::UnsupportedConfig,
        DeviceError::DeviceNotAvailable,
        DeviceError::PermissionDenied,
        DeviceError::BackendFailure,
    ];

    for error in errors {
        let (reporter, reader) = fault_channel();

        reporter.report(error);

        assert_eq!(reader.read(), Some(error));
    }
}
