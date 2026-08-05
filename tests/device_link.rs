//! Holding a stream open across a device that goes away, exercised against a
//! backend with no hardware behind it so that it runs where no audio device
//! exists.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use motif::audio::{
    AudioBackend, AudioState, ChannelSelection, DeviceError, DeviceLink, DeviceSelection,
    DuplexStream, NullBackend, Passthrough, StreamConfig, StreamRequest, StreamState,
};

/// A link over the null backend, playing what it captures.
type Link = DeviceLink<NullBackend, fn() -> Passthrough>;

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

fn one_input_channel() -> DeviceSelection {
    DeviceSelection {
        input_channels: ChannelSelection { first: 1, count: 1 },
        ..selection()
    }
}

fn closed() -> Link {
    DeviceLink::new(
        NullBackend::rounding(config()),
        request(),
        selection(),
        Passthrough::new,
    )
}

fn opened() -> Link {
    let mut link = closed();
    link.open().expect("null backend opens");
    link
}

fn unplug(link: &Link) {
    link.stream()
        .expect("an open link has a stream")
        .fail(DeviceError::DeviceNotAvailable);
}

#[test]
fn a_new_link_has_opened_nothing() {
    let link = closed();

    assert_eq!(link.state(), AudioState::Closed);
}

#[test]
fn an_opened_link_is_idle() {
    assert_eq!(opened().state(), AudioState::Idle);
}

#[test]
fn a_started_link_is_playing() {
    let mut link = opened();

    link.start().expect("null backend starts");

    assert_eq!(link.state(), AudioState::Playing);
}

#[test]
fn a_stopped_link_is_idle_again() {
    let mut link = opened();

    link.start().expect("null backend starts");
    link.stop().expect("null backend stops");

    assert_eq!(link.state(), AudioState::Idle);
}

#[test]
fn a_closed_link_has_no_stream() {
    let mut link = opened();

    link.close();

    assert_eq!(link.state(), AudioState::Closed);
    assert!(link.stream().is_none());
}

#[test]
fn an_open_link_reports_the_configuration_the_device_granted() {
    let link = opened();

    assert_eq!(
        link.stream().map(DuplexStream::config),
        Some(StreamConfig {
            sample_rate: 48_000,
            block_size: 256,
            input_channels: 2,
            output_channels: 2,
        })
    );
}

#[test]
fn a_link_remembers_what_it_was_asked_for() {
    let link = closed();

    assert_eq!(link.request(), request());
}

#[test]
fn a_device_that_goes_away_is_not_noticed_until_the_link_is_polled() {
    let mut link = opened();
    link.start().expect("null backend starts");

    unplug(&link);

    assert_eq!(link.state(), AudioState::Playing);
    assert_eq!(
        link.poll(),
        AudioState::Lost(DeviceError::DeviceNotAvailable)
    );
}

#[test]
fn a_lost_device_stays_lost_across_further_polls() {
    let mut link = opened();
    unplug(&link);

    link.poll();

    assert_eq!(
        link.poll(),
        AudioState::Lost(DeviceError::DeviceNotAvailable)
    );
}

#[test]
fn losing_a_device_tears_the_stream_down() {
    let mut link = opened();
    link.start().expect("null backend starts");
    unplug(&link);

    link.poll();

    assert!(link.stream().is_none());
}

#[test]
fn polling_a_healthy_link_leaves_it_alone() {
    let mut link = opened();
    link.start().expect("null backend starts");

    assert_eq!(link.poll(), AudioState::Playing);
    assert!(link.stream().is_some());
}

#[test]
fn polling_a_link_that_was_never_opened_is_harmless() {
    let mut link = closed();

    assert_eq!(link.poll(), AudioState::Closed);
}

#[test]
fn a_lost_link_refuses_to_start() {
    let mut link = opened();
    unplug(&link);
    link.poll();

    assert_eq!(link.start().err(), Some(DeviceError::DeviceNotAvailable));
}

#[test]
fn a_lost_link_opens_again_without_being_rebuilt() {
    let mut link = opened();
    link.start().expect("null backend starts");
    unplug(&link);
    link.poll();

    link.open().expect("the device came back");

    assert_eq!(link.state(), AudioState::Idle);
}

#[test]
fn a_link_reopened_after_loss_carries_no_fault_from_the_stream_it_replaced() {
    let mut link = opened();
    unplug(&link);
    link.poll();
    link.open().expect("the device came back");

    link.start().expect("the replacement starts");

    assert_eq!(link.poll(), AudioState::Playing);
}

#[test]
fn a_reopen_the_device_refuses_leaves_the_link_lost() {
    let mut link = DeviceLink::new(
        NullBackend::rejecting(config()),
        StreamRequest {
            sample_rate: 44_100,
            block_size: 256,
        },
        selection(),
        Passthrough::new,
    );

    assert_eq!(link.open().err(), Some(DeviceError::UnsupportedConfig));
    assert_eq!(
        link.state(),
        AudioState::Lost(DeviceError::UnsupportedConfig)
    );
}

#[test]
fn opening_an_already_open_link_replaces_its_stream() {
    let mut link = opened();
    link.start().expect("null backend starts");

    link.open().expect("null backend reopens");

    assert_eq!(link.state(), AudioState::Idle);
    assert_eq!(
        link.stream().map(DuplexStream::state),
        Some(StreamState::Stopped)
    );
}

#[test]
fn a_link_remembers_the_selection_it_was_given() {
    let link = closed();

    assert_eq!(link.selection(), &selection());
}

#[test]
fn a_running_link_takes_a_different_selection_without_being_rebuilt() {
    let mut link = opened();
    link.start().expect("null backend starts");

    link.select(one_input_channel())
        .expect("one channel of two opens");

    assert_eq!(link.state(), AudioState::Idle);
    assert_eq!(link.selection(), &one_input_channel());
}

#[test]
fn a_reselection_replaces_the_stream_that_was_running() {
    let mut link = opened();
    link.start().expect("null backend starts");

    link.select(one_input_channel())
        .expect("one channel of two opens");

    assert_eq!(
        link.stream().map(DuplexStream::state),
        Some(StreamState::Stopped)
    );
}

#[test]
fn a_selection_the_device_refuses_leaves_the_link_lost() {
    let mut link = opened();

    let refused = link.select(DeviceSelection {
        input_channels: ChannelSelection { first: 2, count: 2 },
        ..selection()
    });

    assert_eq!(refused.err(), Some(DeviceError::UnsupportedConfig));
    assert_eq!(
        link.state(),
        AudioState::Lost(DeviceError::UnsupportedConfig)
    );
}

#[test]
fn a_refused_selection_is_the_one_the_link_kept() {
    let mut link = opened();
    let refused = DeviceSelection {
        input_channels: ChannelSelection { first: 2, count: 2 },
        ..selection()
    };

    let _ = link.select(refused.clone());

    assert_eq!(link.selection(), &refused);
}

#[test]
fn reopening_uses_the_selection_the_link_was_last_given() {
    let mut link = opened();
    link.select(one_input_channel())
        .expect("one channel of two opens");
    link.close();

    link.open().expect("the same selection opens again");

    assert_eq!(link.selection(), &one_input_channel());
    assert_eq!(link.state(), AudioState::Idle);
}

#[test]
fn a_link_lends_the_backend_it_opens_through() {
    let link = opened();

    let listed = link.backend().hosts(48_000);

    assert_eq!(listed, NullBackend::rounding(config()).hosts(48_000));
}

#[test]
fn an_audio_state_describes_itself() {
    assert_eq!(AudioState::Playing.to_string(), "playing");
}

#[test]
fn a_lost_state_describes_what_went_wrong_with_it() {
    assert_eq!(
        AudioState::Lost(DeviceError::DeviceNotAvailable).to_string(),
        "lost: the device is not available"
    );
}

#[test]
fn every_stream_a_link_opens_gets_a_path_of_its_own() {
    let built = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&built);
    let mut link = DeviceLink::new(
        NullBackend::rounding(config()),
        request(),
        selection(),
        move || {
            counted.fetch_add(1, Ordering::Relaxed);
            Passthrough::new()
        },
    );

    link.open().expect("null backend opens");
    link.open().expect("null backend reopens");

    assert_eq!(built.load(Ordering::Relaxed), 2);
}
