//! Holding a stream open across a device that goes away, exercised against a
//! backend with no hardware behind it so that it runs where no audio device
//! exists.
//!
//! A run has one device and so one link, which the parts that reach it share.
//! What the shared handle says is that sharing one is not copying one: what is
//! opened through either handle is the stream both of them see.
//!
//! Opening happens away from the caller, so a device that takes its time is a
//! device a test can hold inside `open` and look at the link while it waits.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, ThreadId};
use std::time::Duration;

use motif::audio::{
    AudioBackend, AudioHost, AudioPath, AudioState, ChannelSelection, Command, DeviceError,
    DeviceLink, DeviceSelection, DuplexStream, GUARDED_LEVEL, NullBackend, NullStream, Passthrough,
    SharedLink, StreamConfig, StreamRequest, StreamState,
};

/// How long a held device waits before opening itself, where the test that
/// held it has gone without letting it out.
const ABANDONED: Duration = Duration::from_secs(5);

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
    link.open();
    link.settled();
    link
}

fn unplug(link: &Link) {
    link.stream()
        .expect("an open link has a stream")
        .fail(DeviceError::DeviceNotAvailable);
}

/// A device that stays inside `open` until a test lets it out.
///
/// It stands in for a host that serialises a route change, which is the case
/// the bench exists for: everything a caller can see while the device has not
/// answered is what a frame drawn during an open sees.
struct Held {
    entering: Mutex<SyncSender<()>>,
    release: Mutex<Receiver<()>>,
    refuses: bool,
}

/// The end of a held device a test drives it from.
struct Latch {
    entered: Receiver<()>,
    release: SyncSender<()>,
}

impl Latch {
    fn inside(&self) {
        self.entered.recv().expect("the device is being opened");
    }

    fn let_go(&self) {
        self.release.send(()).expect("the device is waiting");
    }
}

fn held(refuses: bool) -> (Held, Latch) {
    let (entering, entered) = sync_channel(1);
    let (release, waiting) = sync_channel(1);

    (
        Held {
            entering: Mutex::new(entering),
            release: Mutex::new(waiting),
            refuses,
        },
        Latch { entered, release },
    )
}

fn locked<T>(guarded: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    guarded.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AudioBackend for Held {
    type Stream = NullStream;

    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost> {
        NullBackend::rounding(config()).hosts(sample_rate)
    }

    fn defaults(&self, sample_rate: u32) -> Option<DeviceSelection> {
        NullBackend::rounding(config()).defaults(sample_rate)
    }

    fn open<P: AudioPath>(
        &self,
        selection: &DeviceSelection,
        request: StreamRequest,
        path: P,
    ) -> Result<Self::Stream, DeviceError> {
        let _entered = locked(&self.entering).try_send(());
        let _released = locked(&self.release).recv_timeout(ABANDONED);

        if self.refuses {
            return Err(DeviceError::DeviceNotAvailable);
        }

        NullBackend::rounding(config()).open(selection, request, path)
    }
}

/// A link over a device a test holds inside `open`.
type Slow = DeviceLink<Held, fn() -> Passthrough>;

fn opening(refuses: bool) -> (Slow, Latch) {
    let (device, latch) = held(refuses);
    let mut link = DeviceLink::new(
        device,
        request(),
        selection(),
        Passthrough::new as fn() -> Passthrough,
    );

    link.open();
    latch.inside();

    (link, latch)
}

/// A path that says which thread it was dropped on.
///
/// A stream owns the path it plays, so where the path was dropped is where the
/// stream it belonged to was torn down.
struct Noting(Arc<Mutex<Option<ThreadId>>>);

impl AudioPath for Noting {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

impl Drop for Noting {
    fn drop(&mut self) {
        *locked(&self.0) = Some(thread::current().id());
    }
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

    link.open();
    link.settled();

    assert_eq!(link.state(), AudioState::Idle);
}

#[test]
fn a_link_reopened_after_loss_carries_no_fault_from_the_stream_it_replaced() {
    let mut link = opened();
    unplug(&link);
    link.poll();
    link.open();
    link.settled();

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

    link.open();

    assert_eq!(
        link.settled(),
        AudioState::Lost(DeviceError::UnsupportedConfig)
    );
}

#[test]
fn a_reopened_link_plays_on_through_the_stream_that_replaced_it() {
    let mut link = opened();
    link.start().expect("null backend starts");

    link.open();

    assert_eq!(link.settled(), AudioState::Playing);
    assert_eq!(
        link.stream().map(DuplexStream::state),
        Some(StreamState::Running)
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

    link.select(one_input_channel());

    assert_eq!(link.settled(), AudioState::Playing);
    assert_eq!(link.selection(), &one_input_channel());
}

#[test]
fn a_reselection_replaces_the_stream_that_was_running() {
    let dropped = Arc::new(Mutex::new(None));
    let mut link = noting(&dropped);
    link.open();
    link.settled();
    link.start().expect("null backend starts");

    link.select(one_input_channel());
    link.settled();

    assert!(locked(&dropped).is_some());
}

#[test]
fn a_chosen_selection_is_not_opened_yet() {
    let mut link = opened();
    link.start().expect("null backend starts");

    link.choose(one_input_channel());

    assert_eq!(link.state(), AudioState::Opening);
    assert_eq!(link.selection(), &one_input_channel());
}

#[test]
fn a_device_being_chosen_keeps_the_stream_that_is_running() {
    let mut link = opened();
    link.start().expect("null backend starts");

    link.choose(one_input_channel());

    assert_eq!(
        link.stream().map(DuplexStream::state),
        Some(StreamState::Running)
    );
}

#[test]
fn opening_is_what_leaves_the_chosen_state() {
    let mut link = opened();
    link.choose(one_input_channel());

    link.open();
    link.settled();

    assert_eq!(link.state(), AudioState::Idle);
    assert_eq!(link.selection(), &one_input_channel());
}

#[test]
fn selecting_is_choosing_and_opening_together() {
    let mut chosen = opened();
    let mut selected = opened();

    chosen.choose(one_input_channel());
    chosen.open();
    chosen.settled();
    selected.select(one_input_channel());
    selected.settled();

    assert_eq!(chosen.state(), selected.state());
    assert_eq!(chosen.selection(), selected.selection());
}

#[test]
fn a_link_that_is_opening_says_so() {
    let mut link = opened();

    link.choose(one_input_channel());

    assert_eq!(link.state().to_string(), "opening");
}

#[test]
fn a_selection_the_device_refuses_leaves_the_link_lost() {
    let mut link = opened();

    link.select(DeviceSelection {
        input_channels: ChannelSelection { first: 2, count: 2 },
        ..selection()
    });

    assert_eq!(
        link.settled(),
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

    link.select(refused.clone());

    assert_eq!(link.selection(), &refused);
}

#[test]
fn reopening_uses_the_selection_the_link_was_last_given() {
    let mut link = opened();
    link.select(one_input_channel());
    link.settled();
    link.close();

    link.open();
    link.settled();

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

    link.open();
    link.settled();
    link.open();
    link.settled();

    assert_eq!(built.load(Ordering::Relaxed), 2);
}

/// A shared link over the null backend, playing what it captures.
type Shared = SharedLink<NullBackend, fn() -> Passthrough>;

fn shared() -> Shared {
    SharedLink::defaulting(
        NullBackend::rounding(config()),
        request(),
        Passthrough::new as fn() -> Passthrough,
    )
    .expect("the null backend has a device in each direction")
}

fn deaf() -> StreamConfig {
    StreamConfig {
        input_channels: 0,
        ..config()
    }
}

#[test]
fn a_shared_link_has_opened_nothing() {
    assert_eq!(shared().read(DeviceLink::state), AudioState::Closed);
}

#[test]
fn a_backend_with_no_default_device_shares_no_link() {
    let none: Option<Shared> = SharedLink::defaulting(
        NullBackend::rounding(deaf()),
        request(),
        Passthrough::new as fn() -> Passthrough,
    );

    assert!(none.is_none());
}

#[test]
fn a_stream_opened_through_one_handle_is_seen_through_the_other() {
    let mut link = shared();
    let watching = link.clone();

    link.change(DeviceLink::open);
    link.change(DeviceLink::settled);

    assert_eq!(watching.read(DeviceLink::state), AudioState::Idle);
}

#[test]
fn a_handle_closed_leaves_the_other_holding_no_stream() {
    let mut link = shared();
    let mut watching = link.clone();
    link.change(DeviceLink::open);
    link.change(DeviceLink::settled);

    watching.change(DeviceLink::close);

    assert_eq!(link.read(DeviceLink::state), AudioState::Closed);
    assert!(link.read(|held| held.stream().is_none()));
}

#[test]
fn a_shared_link_opens_one_stream_however_many_handles_hold_it() {
    let built = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&built);
    let mut link = SharedLink::new(DeviceLink::new(
        NullBackend::rounding(config()),
        request(),
        selection(),
        move || {
            counted.fetch_add(1, Ordering::Relaxed);
            Passthrough::new()
        },
    ));
    let mut watching = link.clone();

    link.change(DeviceLink::open);
    link.change(DeviceLink::settled);
    watching.change(DeviceLink::open);
    watching.change(DeviceLink::settled);

    assert_eq!(built.load(Ordering::Relaxed), 2);
    assert_eq!(link.read(DeviceLink::state), AudioState::Idle);
}

#[test]
fn a_link_nobody_has_chosen_a_device_on_opens_below_unity() {
    assert_eq!(closed().opening_level(), GUARDED_LEVEL);
}

#[test]
fn a_link_a_player_chose_a_device_on_opens_at_unity() {
    let mut link = closed();

    link.select(selection());
    link.settled();

    assert_eq!(link.opening_level(), 1.0);
}

#[test]
fn reopening_a_link_is_not_a_choice() {
    let mut link = opened();

    link.open();
    link.settled();

    assert_eq!(link.opening_level(), GUARDED_LEVEL);
}

#[test]
fn a_chosen_link_stays_at_unity_across_a_device_fault() {
    let mut link = closed();
    link.select(selection());
    link.settled();
    unplug(&link);
    link.poll();

    link.open();
    link.settled();

    assert_eq!(link.opening_level(), 1.0);
}

#[test]
fn a_selection_the_device_refused_is_still_a_choice() {
    let mut link = closed();

    link.select(DeviceSelection::nothing());

    assert_eq!(link.opening_level(), 1.0);
}

const POLLS: usize = 2_000;
const BETWEEN_POLLS: Duration = Duration::from_millis(1);

fn polled_until_answered(link: &mut Slow) -> AudioState {
    for _ in 0..POLLS {
        let state = link.poll();
        if state != AudioState::Opening {
            return state;
        }
        thread::sleep(BETWEEN_POLLS);
    }

    panic!("the device never answered");
}

#[test]
fn an_open_returns_before_the_device_has_answered() {
    let (link, latch) = opening(false);

    assert_eq!(link.state(), AudioState::Opening);

    latch.let_go();
}

#[test]
fn a_link_the_device_has_not_answered_holds_no_stream() {
    let (link, latch) = opening(false);

    assert!(link.stream().is_none());

    latch.let_go();
}

#[test]
fn polling_a_link_the_device_has_not_answered_leaves_it_opening() {
    let (mut link, latch) = opening(false);

    assert_eq!(link.poll(), AudioState::Opening);

    latch.let_go();
}

#[test]
fn settling_waits_for_the_device_and_takes_its_answer() {
    let (mut link, latch) = opening(false);

    latch.let_go();

    assert_eq!(link.settled(), AudioState::Idle);
    assert!(link.stream().is_some());
}

#[test]
fn polling_is_what_takes_the_answer_once_it_arrives() {
    let (mut link, latch) = opening(false);

    latch.let_go();

    assert_eq!(polled_until_answered(&mut link), AudioState::Idle);
}

#[test]
fn a_device_that_refuses_leaves_the_link_lost_when_it_answers() {
    let (mut link, latch) = opening(true);

    latch.let_go();

    assert_eq!(
        link.settled(),
        AudioState::Lost(DeviceError::DeviceNotAvailable)
    );
}

#[test]
fn starting_a_link_the_device_has_not_answered_is_not_a_refusal() {
    let (mut link, latch) = opening(false);

    assert_eq!(link.start(), Ok(()));

    latch.let_go();
}

#[test]
fn a_link_started_while_opening_plays_as_soon_as_the_device_answers() {
    let (mut link, latch) = opening(false);
    link.start().expect("a link being opened takes a start");

    latch.let_go();

    assert_eq!(link.settled(), AudioState::Playing);
}

#[test]
fn a_link_that_was_never_started_is_idle_when_the_device_answers() {
    let (mut link, latch) = opening(false);

    latch.let_go();

    assert_eq!(link.settled(), AudioState::Idle);
}

#[test]
fn a_link_stopped_while_opening_does_not_play_when_the_device_answers() {
    let (mut link, latch) = opening(false);
    link.start().expect("a link being opened takes a start");
    link.stop().expect("a link being opened takes a stop");

    latch.let_go();

    assert_eq!(link.settled(), AudioState::Idle);
}

fn noting(
    where_dropped: &Arc<Mutex<Option<ThreadId>>>,
) -> DeviceLink<NullBackend, impl Fn() -> Noting + Send + Sync + 'static> {
    let noted = Arc::clone(where_dropped);

    DeviceLink::new(
        NullBackend::rounding(config()),
        request(),
        selection(),
        move || Noting(Arc::clone(&noted)),
    )
}

#[test]
fn the_stream_being_replaced_is_torn_down_away_from_the_caller() {
    let dropped = Arc::new(Mutex::new(None));
    let mut link = noting(&dropped);
    link.open();
    link.settled();

    link.open();
    link.settled();

    assert_ne!(*locked(&dropped), Some(thread::current().id()));
    assert!(locked(&dropped).is_some());
}

#[test]
fn a_link_closed_by_hand_tears_its_stream_down_where_it_was_closed() {
    let dropped = Arc::new(Mutex::new(None));
    let mut link = noting(&dropped);
    link.open();
    link.settled();

    link.close();

    assert_eq!(*locked(&dropped), Some(thread::current().id()));
}
