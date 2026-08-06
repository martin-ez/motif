//! The duplex stream lifecycle, exercised against a backend with no hardware
//! behind it so that it runs where no audio device exists.

use motif::audio::{
    AudioBackend, ChannelSelection, DeviceError, DeviceId, DeviceSelection, DuplexStream, Levels,
    NullBackend, Passthrough, Slack, StreamConfig, StreamRequest, StreamState,
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

fn selection() -> DeviceSelection {
    NullBackend::rounding(config())
        .defaults(48_000)
        .expect("the null backend has a device in each direction")
}

#[test]
fn a_stream_is_stopped_before_it_is_started() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    assert_eq!(stream.state(), StreamState::Stopped);
}

#[test]
fn a_started_stream_is_running() {
    let backend = NullBackend::rounding(config());
    let mut stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    stream.start().expect("null backend starts");

    assert_eq!(stream.state(), StreamState::Running);
}

#[test]
fn a_stopped_stream_is_stopped_again() {
    let backend = NullBackend::rounding(config());
    let mut stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    stream.start().expect("null backend starts");
    stream.stop().expect("null backend stops");

    assert_eq!(stream.state(), StreamState::Stopped);
}

#[test]
fn a_stream_can_be_started_again_after_being_stopped() {
    let backend = NullBackend::rounding(config());
    let mut stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    stream.start().expect("null backend starts");
    stream.stop().expect("null backend stops");
    stream.start().expect("null backend restarts");

    assert_eq!(stream.state(), StreamState::Running);
}

#[test]
fn a_stream_that_moves_no_samples_reports_silence() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    assert_eq!(stream.levels(), Levels::SILENT);
}

#[test]
fn a_stream_between_no_two_clocks_holds_no_slack() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    assert_eq!(stream.slack(), Slack::NONE);
}

#[test]
fn the_granted_configuration_is_readable() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    assert_eq!(stream.config(), config());
}

#[test]
fn a_granted_sample_rate_may_differ_from_the_request() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(
            &selection(),
            StreamRequest {
                sample_rate: 44_100,
                block_size: 256,
            },
            Passthrough::new(),
        )
        .expect("a rounding device grants what it has");

    assert_eq!(stream.config().sample_rate, 48_000);
}

#[test]
fn a_granted_block_size_may_differ_from_the_request() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(
            &selection(),
            StreamRequest {
                sample_rate: 48_000,
                block_size: 512,
            },
            Passthrough::new(),
        )
        .expect("a rounding device grants what it has");

    assert_eq!(stream.config().block_size, 256);
}

#[test]
fn a_device_that_cannot_meet_the_sample_rate_is_an_error() {
    let backend = NullBackend::rejecting(config());

    let opened = backend.open(
        &selection(),
        StreamRequest {
            sample_rate: 44_100,
            block_size: 256,
        },
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn a_device_that_cannot_meet_the_block_size_is_an_error() {
    let backend = NullBackend::rejecting(config());

    let opened = backend.open(
        &selection(),
        StreamRequest {
            sample_rate: 48_000,
            block_size: 512,
        },
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn a_rejecting_device_opens_when_the_request_matches_exactly() {
    let backend = NullBackend::rejecting(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("an exact request is met");

    assert_eq!(stream.config(), config());
}

#[test]
fn a_host_the_backend_does_not_have_is_an_error() {
    let backend = NullBackend::rounding(config());

    let opened = backend.open(
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
fn an_input_device_the_host_does_not_have_is_an_error() {
    let backend = NullBackend::rounding(config());

    let opened = backend.open(
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
fn an_output_device_the_host_does_not_have_is_an_error() {
    let backend = NullBackend::rounding(config());

    let opened = backend.open(
        &DeviceSelection {
            output: DeviceId::named("a device nobody has"),
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::NoOutputDevice));
}

#[test]
fn a_selection_reaching_past_the_device_is_an_error() {
    let backend = NullBackend::rounding(config());

    let opened = backend.open(
        &DeviceSelection {
            input_channels: ChannelSelection { first: 1, count: 2 },
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn a_selection_of_no_channels_at_all_is_an_error() {
    let backend = NullBackend::rounding(config());

    let opened = backend.open(
        &DeviceSelection {
            output_channels: ChannelSelection { first: 0, count: 0 },
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

#[test]
fn a_narrower_selection_still_opens_the_device_wide_enough_to_reach_it() {
    let backend = NullBackend::rounding(config());

    let stream = backend
        .open(
            &DeviceSelection {
                input_channels: ChannelSelection { first: 1, count: 1 },
                ..selection()
            },
            request(),
            Passthrough::new(),
        )
        .expect("channel two is inside a two-channel device");

    assert_eq!(stream.config().input_channels, 2);
}

#[test]
fn a_selection_that_reaches_past_the_end_of_a_channel_count_is_an_error() {
    let backend = NullBackend::rounding(config());

    let opened = backend.open(
        &DeviceSelection {
            input_channels: ChannelSelection {
                first: u16::MAX,
                count: 1,
            },
            ..selection()
        },
        request(),
        Passthrough::new(),
    );

    assert_eq!(opened.err(), Some(DeviceError::UnsupportedConfig));
}

fn opened_across(
    natural: u16,
    offers: Vec<u16>,
    input_channels: ChannelSelection,
) -> Result<u16, DeviceError> {
    let backend = NullBackend::offering(
        StreamConfig {
            input_channels: natural,
            ..config()
        },
        offers,
    );
    let chosen = backend
        .defaults(48_000)
        .expect("the null backend has a device in each direction");

    backend
        .open(
            &DeviceSelection {
                input_channels,
                ..chosen
            },
            request(),
            Passthrough::new(),
        )
        .map(|stream| stream.config().input_channels)
}

#[test]
fn a_device_is_opened_at_the_narrowest_width_that_reaches_the_selection() {
    let opened = opened_across(1, vec![1, 2, 4, 8], ChannelSelection { first: 2, count: 1 });

    assert_eq!(opened, Ok(4));
}

#[test]
fn a_device_is_opened_no_narrower_than_the_width_it_runs_at() {
    let opened = opened_across(2, vec![1, 2, 4], ChannelSelection { first: 0, count: 1 });

    assert_eq!(opened, Ok(2));
}

#[test]
fn a_device_offering_nothing_that_wide_is_opened_across_the_selection_alone() {
    let opened = opened_across(8, vec![1, 2], ChannelSelection { first: 0, count: 1 });

    assert_eq!(opened, Ok(1));
}

fn played_across(
    natural: u16,
    offers: Vec<u16>,
    output_channels: ChannelSelection,
) -> Result<u16, DeviceError> {
    let backend = NullBackend::offering(
        StreamConfig {
            output_channels: natural,
            ..config()
        },
        offers,
    );
    let chosen = backend
        .defaults(48_000)
        .expect("the null backend has a device in each direction");

    backend
        .open(
            &DeviceSelection {
                output_channels,
                ..chosen
            },
            request(),
            Passthrough::new(),
        )
        .map(|stream| stream.config().output_channels)
}

#[test]
fn the_output_device_is_opened_at_the_narrowest_width_that_reaches_the_selection() {
    let played = played_across(1, vec![1, 2, 4, 8], ChannelSelection { first: 2, count: 1 });

    assert_eq!(played, Ok(4));
}

#[test]
fn the_output_device_is_opened_no_narrower_than_the_width_it_runs_at() {
    let played = played_across(2, vec![1, 2, 4], ChannelSelection { first: 0, count: 1 });

    assert_eq!(played, Ok(2));
}

#[test]
fn a_stream_whose_device_is_present_reports_no_fault() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

    assert_eq!(stream.fault(), None);
}

#[test]
fn a_stream_whose_device_went_away_reports_the_fault() {
    let backend = NullBackend::rounding(config());
    let stream = backend
        .open(&selection(), request(), Passthrough::new())
        .expect("null backend opens");

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
