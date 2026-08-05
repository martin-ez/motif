//! Telling one device from another that describes itself the same way.
//!
//! Two interfaces of one model, or the `hw:` and `plughw:` entries for one
//! card, arrive as separate devices carrying one name. Exercised against a
//! backend whose devices come in same-named pairs offering different channel
//! counts, so which of the two was opened is visible in what it granted.

use motif::audio::{
    AudioBackend, ChannelSelection, DeviceError, DeviceId, DeviceSelection, DuplexStream,
    NullBackend, StreamConfig, StreamRequest,
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

fn twinned() -> NullBackend {
    NullBackend::twinned(config(), vec![4])
}

fn devices(backend: &NullBackend) -> (Vec<DeviceId>, Vec<DeviceId>) {
    let hosts = backend.hosts(48_000);
    let host = hosts.first().expect("the null backend lists its host");

    (
        host.inputs.iter().map(|device| device.id.clone()).collect(),
        host.outputs
            .iter()
            .map(|device| device.id.clone())
            .collect(),
    )
}

fn selecting(input: DeviceId, output: DeviceId) -> DeviceSelection {
    DeviceSelection {
        input,
        input_channels: ChannelSelection::all(2),
        output,
        output_channels: ChannelSelection::all(2),
        ..twinned()
            .defaults(48_000)
            .expect("the null backend has a device in each direction")
    }
}

#[test]
fn a_name_alone_labels_the_first_device_carrying_it() {
    assert_eq!(DeviceId::named("interface").to_string(), "interface");
}

#[test]
fn a_later_device_of_a_name_is_labelled_with_which_one_it_is() {
    let second = DeviceId {
        name: "interface".to_owned(),
        nth: 1,
    };

    assert_eq!(second.to_string(), "interface (2)");
}

#[test]
fn devices_sharing_a_name_are_listed_as_rows_of_their_own() {
    let (inputs, outputs) = devices(&twinned());

    assert_eq!(inputs.len(), 2);
    assert_eq!(outputs.len(), 2);
}

#[test]
fn devices_sharing_a_name_are_told_apart_by_which_one_they_are() {
    let (inputs, _) = devices(&twinned());

    assert_eq!(inputs[0].name, inputs[1].name);
    assert_eq!(inputs[0].nth, 0);
    assert_eq!(inputs[1].nth, 1);
}

#[test]
fn a_device_lists_the_channel_counts_it_offers_rather_than_its_twin_s() {
    let hosts = twinned().hosts(48_000);

    assert_eq!(hosts[0].inputs[0].channels, vec![2]);
    assert_eq!(hosts[0].inputs[1].channels, vec![4]);
}

#[test]
fn a_selection_taking_the_first_device_of_a_name_opens_that_one() {
    let (inputs, outputs) = devices(&twinned());

    let stream = twinned()
        .open(&selecting(inputs[0].clone(), outputs[0].clone()), request())
        .expect("a listed device opens");

    assert_eq!(stream.config().input_channels, 2);
}

#[test]
fn a_selection_taking_the_second_device_of_a_name_opens_that_one() {
    let (inputs, outputs) = devices(&twinned());

    let stream = twinned()
        .open(&selecting(inputs[1].clone(), outputs[0].clone()), request())
        .expect("a listed device opens");

    assert_eq!(stream.config().input_channels, 4);
}

#[test]
fn the_second_device_of_a_name_is_reachable_in_both_directions() {
    let (inputs, outputs) = devices(&twinned());

    let stream = twinned()
        .open(&selecting(inputs[0].clone(), outputs[1].clone()), request())
        .expect("a listed device opens");

    assert_eq!(stream.config().output_channels, 4);
}

#[test]
fn opening_what_a_listing_offered_reaches_the_device_it_meant() {
    let hosts = twinned().hosts(48_000);
    let host = &hosts[0];

    for input in &host.inputs {
        let stream = twinned()
            .open(
                &selecting(input.id.clone(), host.outputs[0].id.clone()),
                request(),
            )
            .expect("listed means openable");

        assert_eq!(
            stream.config().input_channels,
            input.channels[0],
            "{} granted a width another device offers",
            input.id
        );
    }
}

#[test]
fn a_device_of_a_name_that_was_never_listed_is_not_opened() {
    let (inputs, outputs) = devices(&twinned());
    let past_the_last = DeviceId {
        nth: inputs.len(),
        ..inputs[0].clone()
    };

    let opened = twinned().open(&selecting(past_the_last, outputs[0].clone()), request());

    assert_eq!(opened.err(), Some(DeviceError::NoInputDevice));
}

#[test]
fn an_output_of_a_name_that_was_never_listed_is_not_opened() {
    let (inputs, outputs) = devices(&twinned());
    let past_the_last = DeviceId {
        nth: outputs.len(),
        ..outputs[0].clone()
    };

    let opened = twinned().open(&selecting(inputs[0].clone(), past_the_last), request());

    assert_eq!(opened.err(), Some(DeviceError::NoOutputDevice));
}

#[test]
fn a_default_selection_takes_the_first_device_of_a_name() {
    let selection = twinned()
        .defaults(48_000)
        .expect("the null backend has a device in each direction");

    assert_eq!(selection.input.nth, 0);
    assert_eq!(selection.output.nth, 0);
}
