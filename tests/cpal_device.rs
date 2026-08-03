//! The same lifecycle against real hardware.
//!
//! Ignored by default: continuous integration runners have no audio device, so
//! these are run deliberately with `cargo test -- --ignored` on a machine that
//! has one.

use motif::audio::{AudioBackend, CpalBackend, DuplexStream, StreamRequest, StreamState};

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: 48_000,
        block_size: 256,
    }
}

#[test]
#[ignore = "requires an audio device"]
fn a_device_grants_a_configuration_it_can_run() {
    let backend = CpalBackend::new();
    let stream = backend.open(request()).expect("a default device opens");

    let config = stream.config();
    assert_eq!(config.sample_rate, 48_000);
    assert!(config.block_size > 0);
    assert!(config.input_channels > 0);
    assert!(config.output_channels > 0);
}

#[test]
#[ignore = "requires an audio device"]
fn a_device_stream_starts_and_stops() {
    let backend = CpalBackend::new();
    let mut stream = backend.open(request()).expect("a default device opens");

    assert_eq!(stream.state(), StreamState::Stopped);
    stream.start().expect("a default device starts");
    assert_eq!(stream.state(), StreamState::Running);
    stream.stop().expect("a default device stops");
    assert_eq!(stream.state(), StreamState::Stopped);
}

#[test]
#[ignore = "requires an audio device"]
fn a_sample_rate_no_device_supports_is_an_error() {
    let backend = CpalBackend::new();

    let opened = backend.open(StreamRequest {
        sample_rate: 1,
        block_size: 256,
    });

    assert!(opened.is_err());
}
