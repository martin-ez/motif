//! The duplex stream lifecycle, exercised against a backend with no hardware
//! behind it so that it runs where no audio device exists.

use motif::audio::{
    AudioBackend, DeviceError, DuplexStream, Levels, NullBackend, StreamConfig, StreamRequest,
    StreamState,
};

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 256,
        input_channels: 2,
        output_channels: 2,
    }
}

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: 48_000,
        block_size: 256,
    }
}

#[test]
fn a_stream_is_stopped_before_it_is_started() {
    let backend = NullBackend::rounding(config());
    let stream = backend.open(request()).expect("null backend opens");

    assert_eq!(stream.state(), StreamState::Stopped);
}

#[test]
fn a_started_stream_is_running() {
    let backend = NullBackend::rounding(config());
    let mut stream = backend.open(request()).expect("null backend opens");

    stream.start().expect("null backend starts");

    assert_eq!(stream.state(), StreamState::Running);
}

#[test]
fn a_stopped_stream_is_stopped_again() {
    let backend = NullBackend::rounding(config());
    let mut stream = backend.open(request()).expect("null backend opens");

    stream.start().expect("null backend starts");
    stream.stop().expect("null backend stops");

    assert_eq!(stream.state(), StreamState::Stopped);
}

#[test]
fn a_stream_can_be_started_again_after_being_stopped() {
    let backend = NullBackend::rounding(config());
    let mut stream = backend.open(request()).expect("null backend opens");

    stream.start().expect("null backend starts");
    stream.stop().expect("null backend stops");
    stream.start().expect("null backend restarts");

    assert_eq!(stream.state(), StreamState::Running);
}

#[test]
fn a_stream_that_moves_no_samples_reports_silence() {
    let backend = NullBackend::rounding(config());
    let stream = backend.open(request()).expect("null backend opens");

    assert_eq!(stream.levels(), Levels::SILENT);
}

#[test]
fn the_granted_configuration_is_readable() {
    let backend = NullBackend::rounding(config());
    let stream = backend.open(request()).expect("null backend opens");

    assert_eq!(stream.config(), config());
}

#[test]
fn a_granted_sample_rate_may_differ_from_the_request() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(StreamRequest {
            sample_rate: 44_100,
            block_size: 256,
        })
        .expect("a rounding device grants what it has");

    assert_eq!(stream.config().sample_rate, 48_000);
}

#[test]
fn a_granted_block_size_may_differ_from_the_request() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(StreamRequest {
            sample_rate: 48_000,
            block_size: 512,
        })
        .expect("a rounding device grants what it has");

    assert_eq!(stream.config().block_size, 256);
}

#[test]
fn a_device_that_cannot_meet_the_sample_rate_is_an_error() {
    let backend = NullBackend::rejecting(config());

    let opened = backend.open(StreamRequest {
        sample_rate: 44_100,
        block_size: 256,
    });

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn a_device_that_cannot_meet_the_block_size_is_an_error() {
    let backend = NullBackend::rejecting(config());

    let opened = backend.open(StreamRequest {
        sample_rate: 48_000,
        block_size: 512,
    });

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn a_rejecting_device_opens_when_the_request_matches_exactly() {
    let backend = NullBackend::rejecting(config());
    let stream = backend.open(request()).expect("an exact request is met");

    assert_eq!(stream.config(), config());
}

#[test]
fn a_stream_whose_device_is_present_reports_no_fault() {
    let backend = NullBackend::rounding(config());
    let stream = backend.open(request()).expect("null backend opens");

    assert_eq!(stream.fault(), None);
}

#[test]
fn a_stream_whose_device_went_away_reports_the_fault() {
    let backend = NullBackend::rounding(config());
    let stream = backend.open(request()).expect("null backend opens");

    stream.fail(DeviceError::DeviceNotAvailable);

    assert_eq!(stream.fault(), Some(DeviceError::DeviceNotAvailable));
}

#[test]
fn a_device_error_describes_itself() {
    assert_eq!(
        DeviceError::UnsupportedConfig.to_string(),
        "the device cannot run at the requested configuration"
    );
}
