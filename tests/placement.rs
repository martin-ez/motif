//! Putting the audio callback on a core of its own, and reporting what the
//! host allowed.
//!
//! The facts worth stating are which core is reserved, that an error number is
//! read as the answer it is, that the thread doing the pinning gets its own
//! placement back, and that a host with no way to place a thread leaves it
//! running rather than taking the program down with it.
//!
//! Two of these need an audio device and are ignored by default, as the rest of
//! the hardware tests are. One is a measurement: it holds the callback to its
//! deadline under load, and the comparison it is for is the same run on `main`.

use std::thread;
use std::time::{Duration, Instant};

use motif::audio::{
    AudioBackend, CpalBackend, DeviceSelection, DuplexStream, Grant, HOSTED_PRIORITY, NullBackend,
    Passthrough, Placed, Placement, StreamConfig, StreamRequest, pinning, priority_latch,
};
use motif::device::DeviceProfile;

fn granted() -> StreamConfig {
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
    NullBackend::rounding(granted())
        .defaults(48_000)
        .expect("the null backend has a device in each direction")
}

/// The core the target profile reserves, which is the last of its four.
const RESERVED: usize = 3;

/// A core a host can name and no host has, which is the refusal a host makes
/// rather than the one the mask makes for it.
const UNBUILT: usize = 1000;

fn placed_on(core: usize) -> Grant {
    thread::spawn(move || pinning(Placement { core }, || {}).0)
        .join()
        .expect("a placed thread runs to completion")
}

#[test]
fn the_target_profile_reserves_its_last_core() {
    assert_eq!(
        Placement::reserving_last_of(DeviceProfile::TARGET.cores).core,
        RESERVED
    );
}

#[test]
fn one_core_is_reserved_as_readily_as_four() {
    assert_eq!(Placement::reserving_last_of(1).core, 0);
}

#[test]
fn a_machine_with_no_cores_reserves_the_first() {
    assert_eq!(Placement::reserving_last_of(0).core, 0);
}

#[test]
fn the_core_available_is_one_this_process_may_run_on() {
    let available = Placement::available();

    assert_eq!(placed_on(available.core), placed_on(available.core));
    assert_ne!(placed_on(available.core), Grant::Unasked);
}

#[test]
fn a_permission_answer_is_a_refusal_and_anything_else_is_not() {
    assert_eq!(Grant::refusing(libc::EPERM), Grant::Refused);
    assert_eq!(Grant::refusing(libc::EACCES), Grant::Refused);
    assert_eq!(Grant::refusing(libc::EINVAL), Grant::Unavailable);
    assert_eq!(Grant::refusing(libc::ENOTSUP), Grant::Unavailable);
}

#[test]
fn a_priority_latch_reads_what_the_layer_below_gives_until_it_is_denied() {
    let (reporter, reader) = priority_latch();

    assert_eq!(reader.read(), HOSTED_PRIORITY);

    reporter.denied();

    assert_eq!(reader.read(), Grant::Refused);
    assert_eq!(reader.read(), Grant::Refused);
}

#[test]
fn a_core_no_host_has_is_refused_rather_than_fatal() {
    for core in [UNBUILT, usize::MAX] {
        let granted = placed_on(core);

        assert_ne!(granted, Grant::Given);
        assert_ne!(granted, Grant::Unasked);
    }
}

#[test]
fn pinning_puts_back_the_mask_it_borrowed() {
    let before = Placement::available();

    let (_, during) = pinning(Placement { core: 0 }, Placement::available);

    assert_eq!(Placement::available(), before);
    assert_ne!(during, Placement { core: usize::MAX });
}

#[test]
fn whatever_is_built_while_pinned_comes_back() {
    let (_, built) = pinning(Placement { core: 0 }, || "a stream");

    assert_eq!(built, "a stream");
}

#[cfg(target_os = "linux")]
#[test]
fn a_linux_host_pins_the_thread_to_a_core_it_already_owns() {
    assert_eq!(placed_on(Placement::available().core), Grant::Given);
}

#[cfg(target_os = "linux")]
#[test]
fn a_linux_host_has_no_core_for_one_it_has_no_bit_or_no_silicon_for() {
    assert_eq!(placed_on(UNBUILT), Grant::Unavailable);
    assert_eq!(placed_on(usize::MAX), Grant::Unavailable);
}

#[cfg(target_os = "macos")]
#[test]
fn a_macos_host_offers_no_way_to_pin_a_thread() {
    assert_eq!(placed_on(0), Grant::Unavailable);
}

#[test]
fn a_stream_with_no_callback_thread_is_unasked() {
    let stream = NullBackend::rounding(granted())
        .open(&selection(), request(), Passthrough::new())
        .expect("the null backend opens its own device");

    assert_eq!(stream.placement(), Placed::UNASKED);
}

fn device() -> DeviceSelection {
    CpalBackend::new()
        .defaults(48_000)
        .expect("a machine with an audio device has a default one")
}

#[test]
#[ignore = "requires an audio device"]
fn a_running_stream_reports_where_its_callback_went() {
    let mut stream = CpalBackend::new()
        .open(&device(), request(), Passthrough::new())
        .expect("a default device opens");

    stream.start().expect("a default device starts");
    thread::sleep(Duration::from_millis(200));

    let placed = stream.placement();
    stream.stop().expect("a default device stops");

    assert_ne!(placed.affinity, Grant::Unasked);
    assert_ne!(placed.priority, Grant::Unasked);
}

/// How long the measurement runs the stream for, which is long enough for a
/// scheduler to have had chances to move the callback and taken them.
const MEASURED_FOR: Duration = Duration::from_secs(10);

/// What a passthrough is allowed to take of its deadline before the placement
/// is not buying what it was added to buy.
///
/// A passthrough copies a block and measures it; it should not come close, and
/// a run that does is reporting contention rather than work.
const SPARE_UNDER_LOAD: f32 = 0.5;

fn spinners(until: Instant) -> Vec<thread::JoinHandle<()>> {
    (0..DeviceProfile::TARGET.cores * 2)
        .map(|_| thread::spawn(move || while Instant::now() < until {}))
        .collect()
}

#[test]
#[ignore = "requires an audio device"]
fn a_loaded_host_leaves_the_callback_its_headroom() {
    let mut stream = CpalBackend::new()
        .open(&device(), request(), Passthrough::new())
        .expect("a default device opens");

    stream.start().expect("a default device starts");
    for spinner in spinners(Instant::now() + MEASURED_FOR) {
        spinner.join().expect("a spinner runs to completion");
    }

    let headroom = stream.headroom();
    let xruns = stream.xruns();
    let placed = stream.placement();
    stream.stop().expect("a default device stops");

    println!("placement {placed:?}");
    println!(
        "peak load {:.3}, spare {:.3}",
        headroom.peak,
        headroom.spare()
    );
    println!("overruns {}, underruns {}", xruns.overruns, xruns.underruns);

    assert!(
        headroom.spare() > SPARE_UNDER_LOAD,
        "the callback kept {:.3} of its deadline spare under load",
        headroom.spare()
    );
    assert_eq!(xruns.underruns, 0);
}
