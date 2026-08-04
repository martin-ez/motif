//! What a backend says there is to open, exercised against a backend with no
//! hardware behind it so that it runs where no audio device exists.

use motif::audio::{AudioBackend, NullBackend, StreamConfig};

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: 48_000,
        block_size: 256,
        input_channels: 2,
        output_channels: 2,
    }
}

#[test]
fn a_backend_lists_the_host_its_devices_are_on() {
    let backend = NullBackend::rounding(config());

    let hosts = backend.hosts(48_000);

    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].inputs.len(), 1);
    assert_eq!(hosts[0].outputs.len(), 1);
}

#[test]
fn a_listed_device_reports_the_channel_counts_it_can_open() {
    let backend = NullBackend::rounding(config());

    let hosts = backend.hosts(48_000);

    assert_eq!(hosts[0].inputs[0].channels, vec![2]);
    assert_eq!(hosts[0].outputs[0].channels, vec![2]);
}

#[test]
fn a_listed_device_is_named() {
    let backend = NullBackend::rounding(config());

    let hosts = backend.hosts(48_000);

    assert!(!hosts[0].name.is_empty());
    assert!(!hosts[0].inputs[0].name.is_empty());
    assert!(!hosts[0].outputs[0].name.is_empty());
}

#[test]
fn a_device_is_listed_only_at_the_rate_it_was_granted() {
    let backend = NullBackend::rounding(config());

    let hosts = backend.hosts(44_100);

    assert_eq!(hosts, Vec::new());
}

#[test]
fn a_direction_the_device_has_no_channels_for_is_absent() {
    let backend = NullBackend::rounding(StreamConfig {
        input_channels: 0,
        ..config()
    });

    let hosts = backend.hosts(48_000);

    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].inputs, Vec::new());
    assert_eq!(hosts[0].outputs.len(), 1);
}

#[test]
fn a_host_with_no_devices_at_all_is_absent() {
    let backend = NullBackend::rounding(StreamConfig {
        input_channels: 0,
        output_channels: 0,
        ..config()
    });

    let hosts = backend.hosts(48_000);

    assert_eq!(hosts, Vec::new());
}
