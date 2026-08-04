//! The same lifecycle against real hardware.
//!
//! Ignored by default: continuous integration runners have no audio device, so
//! these are run deliberately with `cargo test -- --ignored` on a machine that
//! has one.

use motif::audio::{
    AudioBackend, AudioDevice, CpalBackend, DuplexStream, StreamRequest, StreamState,
};

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
fn a_block_size_of_zero_is_an_error_rather_than_a_panic() {
    let backend = CpalBackend::new();

    let opened = backend.open(StreamRequest {
        sample_rate: 48_000,
        block_size: 0,
    });

    assert!(opened.is_err());
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

fn listed_devices(sample_rate: u32) -> Vec<AudioDevice> {
    CpalBackend::new()
        .hosts(sample_rate)
        .into_iter()
        .flat_map(|host| host.inputs.into_iter().chain(host.outputs))
        .collect()
}

#[test]
#[ignore = "requires an audio device"]
fn a_machine_with_a_device_lists_a_host_to_reach_it_through() {
    let hosts = CpalBackend::new().hosts(48_000);

    assert!(!hosts.is_empty());
    assert!(hosts.iter().all(|host| !host.name.is_empty()));
}

#[test]
#[ignore = "requires an audio device"]
fn every_listed_device_offers_a_channel_count() {
    for device in listed_devices(48_000) {
        assert!(!device.name.is_empty());
        assert!(!device.channels.is_empty(), "{} lists none", device.name);
    }
}

#[test]
#[ignore = "requires an audio device"]
fn channel_counts_ascend_without_repeats() {
    for device in listed_devices(48_000) {
        assert!(
            device.channels.windows(2).all(|pair| pair[0] < pair[1]),
            "{} lists {:?}",
            device.name,
            device.channels
        );
    }
}

#[test]
#[ignore = "requires an audio device"]
fn a_sample_rate_no_device_supports_lists_no_hosts() {
    assert_eq!(CpalBackend::new().hosts(1), Vec::new());
}
