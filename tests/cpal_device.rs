//! The same lifecycle against real hardware.
//!
//! Ignored by default: continuous integration runners have no audio device, so
//! these are run deliberately with `cargo test -- --ignored` on a machine that
//! has one.

use motif::audio::{
    AudioBackend, AudioDevice, ChannelSelection, CpalBackend, DeviceError, DeviceId,
    DeviceSelection, DuplexStream, Passthrough, StreamRequest, StreamState,
};

fn request() -> StreamRequest {
    StreamRequest {
        sample_rate: 48_000,
        block_size: 256,
    }
}

fn selection() -> DeviceSelection {
    CpalBackend::new()
        .defaults(48_000)
        .expect("a machine with an audio device has a default one")
}

#[test]
#[ignore = "requires an audio device"]
fn a_device_grants_a_configuration_it_can_run() {
    let backend = CpalBackend::new();
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("a default device opens");

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
    let mut stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("a default device opens");

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

    let opened = backend.open(
        &selection(),
        StreamRequest {
            sample_rate: 48_000,
            block_size: 0,
        },
        Passthrough::new(),
    );

    assert!(opened.is_err());
}

#[test]
#[ignore = "requires an audio device"]
fn a_sample_rate_no_device_supports_is_an_error() {
    let backend = CpalBackend::new();

    let opened = backend.open(
        &selection(),
        StreamRequest {
            sample_rate: 1,
            block_size: 256,
        },
        Passthrough::new(),
    );

    assert!(opened.is_err());
}

#[test]
#[ignore = "requires an audio device"]
fn a_default_selection_names_devices_the_backend_lists() {
    let chosen = selection();
    let hosts = CpalBackend::new().hosts(48_000);

    let host = hosts
        .iter()
        .find(|host| host.name == chosen.host)
        .expect("the default host is listed");

    assert!(host.inputs.iter().any(|device| device.id == chosen.input));
    assert!(host.outputs.iter().any(|device| device.id == chosen.output));
}

#[test]
#[ignore = "requires an audio device"]
fn a_device_opens_against_a_selection_taken_from_the_listing() {
    let hosts = CpalBackend::new().hosts(48_000);
    let host = hosts
        .iter()
        .find(|host| !host.inputs.is_empty() && !host.outputs.is_empty())
        .expect("a machine with an audio device has a host with both");

    let stream = CpalBackend::new().open(
        &DeviceSelection {
            host: host.name.clone(),
            input: host.inputs[0].id.clone(),
            input_channels: ChannelSelection::all(host.inputs[0].channels[0]),
            output: host.outputs[0].id.clone(),
            output_channels: ChannelSelection::all(host.outputs[0].channels[0]),
        },
        request(),
        Passthrough::new(),
    );

    assert!(stream.is_ok(), "listed means openable");
}

#[test]
#[ignore = "requires an audio device"]
fn a_host_no_backend_has_is_an_error_rather_than_a_default() {
    let opened = CpalBackend::new().open(
        &DeviceSelection {
            host: "a host nobody has".to_owned(),
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::NoSuchHost));
}

#[test]
#[ignore = "requires an audio device"]
fn a_device_no_host_has_is_an_error_rather_than_a_default() {
    let opened = CpalBackend::new().open(
        &DeviceSelection {
            input: DeviceId::named("a device nobody has"),
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::NoInputDevice));
}

#[test]
#[ignore = "requires an audio device"]
fn a_selection_reaching_past_the_device_is_an_error() {
    let opened = CpalBackend::new().open(
        &DeviceSelection {
            input_channels: ChannelSelection {
                first: u16::MAX - 1,
                count: 1,
            },
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
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
        assert!(!device.id.name.is_empty());
        assert!(!device.channels.is_empty(), "{} lists none", device.id);
    }
}

#[test]
#[ignore = "requires an audio device"]
fn channel_counts_ascend_without_repeats() {
    for device in listed_devices(48_000) {
        assert!(
            device.channels.windows(2).all(|pair| pair[0] < pair[1]),
            "{} lists {:?}",
            device.id,
            device.channels
        );
    }
}

#[test]
#[ignore = "requires an audio device"]
fn no_two_devices_of_one_host_and_direction_share_an_identity() {
    for host in CpalBackend::new().hosts(48_000) {
        for direction in [host.inputs, host.outputs] {
            let mut identities: Vec<DeviceId> = Vec::new();
            for device in direction {
                assert!(!identities.contains(&device.id), "{} twice", device.id);
                identities.push(device.id);
            }
        }
    }
}

#[test]
#[ignore = "requires an audio device"]
fn a_sample_rate_no_device_supports_lists_no_hosts() {
    assert_eq!(CpalBackend::new().hosts(1), Vec::new());
}
