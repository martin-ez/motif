//! What a page draws between deliberate refreshes, and what a refresh is
//! allowed to take away.
//!
//! Exercised against a backend that counts what it was asked and can be told to
//! report a different listing, which is how a device going busy is modelled
//! where there is no hardware to make busy.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use motif::audio::{
    AudioBackend, AudioDevice, AudioHost, AudioPath, ChannelSelection, DeviceCatalog, DeviceError,
    DeviceId, DeviceSelection, NullStream, StreamRequest,
};

struct CountingBackend {
    listing: Mutex<Vec<AudioHost>>,
    enumerations: AtomicUsize,
    asked_at: AtomicU32,
}

impl CountingBackend {
    fn new(listing: Vec<AudioHost>) -> Self {
        Self {
            listing: Mutex::new(listing),
            enumerations: AtomicUsize::new(0),
            asked_at: AtomicU32::new(0),
        }
    }

    fn now_lists(&self, listing: Vec<AudioHost>) {
        *self.held() = listing;
    }

    fn enumerations(&self) -> usize {
        self.enumerations.load(Ordering::Relaxed)
    }

    fn asked_at(&self) -> u32 {
        self.asked_at.load(Ordering::Relaxed)
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Vec<AudioHost>> {
        self.listing.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl AudioBackend for CountingBackend {
    type Stream = NullStream;

    fn hosts(&self, sample_rate: u32) -> Vec<AudioHost> {
        self.enumerations.fetch_add(1, Ordering::Relaxed);
        self.asked_at.store(sample_rate, Ordering::Relaxed);
        self.held().clone()
    }

    fn defaults(&self, _sample_rate: u32) -> Option<DeviceSelection> {
        None
    }

    fn open<P: AudioPath>(
        &self,
        _selection: &DeviceSelection,
        _request: StreamRequest,
        _path: P,
    ) -> Result<Self::Stream, DeviceError> {
        Err(DeviceError::DeviceNotAvailable)
    }
}

fn device(name: &str, channels: Vec<u16>) -> AudioDevice {
    AudioDevice {
        id: DeviceId::named(name),
        channels,
    }
}

fn second_of(name: &str, channels: Vec<u16>) -> AudioDevice {
    AudioDevice {
        id: DeviceId {
            name: name.to_owned(),
            nth: 1,
        },
        channels,
    }
}

fn one_host() -> Vec<AudioHost> {
    vec![AudioHost {
        name: "alsa".to_owned(),
        inputs: vec![device("interface", vec![2, 4]), device("webcam", vec![1])],
        outputs: vec![device("interface", vec![2])],
    }]
}

fn without_the_interface() -> Vec<AudioHost> {
    vec![AudioHost {
        name: "alsa".to_owned(),
        inputs: vec![device("webcam", vec![1])],
        outputs: Vec::new(),
    }]
}

fn without_the_interface_output() -> Vec<AudioHost> {
    vec![AudioHost {
        name: "alsa".to_owned(),
        inputs: vec![device("interface", vec![2, 4]), device("webcam", vec![1])],
        outputs: Vec::new(),
    }]
}

fn held() -> DeviceSelection {
    DeviceSelection {
        host: "alsa".to_owned(),
        input: DeviceId::named("interface"),
        input_channels: ChannelSelection::all(2),
        output: DeviceId::named("interface"),
        output_channels: ChannelSelection::all(2),
    }
}

fn two_interfaces() -> Vec<AudioHost> {
    vec![AudioHost {
        name: "alsa".to_owned(),
        inputs: vec![
            device("interface", vec![2, 4]),
            second_of("interface", vec![2]),
        ],
        outputs: vec![device("interface", vec![2])],
    }]
}

fn holding_the_second_interface() -> DeviceSelection {
    DeviceSelection {
        input: DeviceId {
            name: "interface".to_owned(),
            nth: 1,
        },
        ..held()
    }
}

fn input_ids(catalog: &DeviceCatalog) -> Vec<DeviceId> {
    catalog
        .hosts()
        .iter()
        .flat_map(|host| host.inputs.iter())
        .map(|device| device.id.clone())
        .collect()
}

fn input_names(catalog: &DeviceCatalog) -> Vec<String> {
    input_ids(catalog)
        .iter()
        .map(|id| id.name.clone())
        .collect()
}

#[test]
fn a_new_catalog_has_nothing_to_draw() {
    let catalog = DeviceCatalog::new(48_000);

    assert_eq!(catalog.hosts(), &[]);
}

#[test]
fn a_new_catalog_has_listed_nothing() {
    let catalog = DeviceCatalog::new(48_000);

    assert!(!catalog.has_listed());
}

#[test]
fn a_refreshed_catalog_has_listed() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);

    catalog.refresh(&backend, None);

    assert!(catalog.has_listed());
}

#[test]
fn a_catalog_that_listed_nothing_has_still_listed() {
    let backend = CountingBackend::new(Vec::new());
    let mut catalog = DeviceCatalog::new(48_000);

    catalog.refresh(&backend, None);

    assert!(catalog.has_listed());
}

#[test]
fn a_refreshed_catalog_lists_what_the_backend_gave() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);

    catalog.refresh(&backend, None);

    assert_eq!(catalog.hosts(), one_host().as_slice());
}

#[test]
fn a_catalog_asks_at_the_rate_it_was_built_for() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(44_100);

    catalog.refresh(&backend, None);

    assert_eq!(backend.asked_at(), 44_100);
}

#[test]
fn reading_the_listing_does_not_reach_the_backend() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    for _ in 0..100 {
        let _ = catalog.hosts();
    }

    assert_eq!(backend.enumerations(), 1);
}

#[test]
fn each_refresh_reaches_the_backend_once() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);

    catalog.refresh(&backend, None);
    catalog.refresh(&backend, None);
    catalog.refresh(&backend, None);

    assert_eq!(backend.enumerations(), 3);
}

#[test]
fn a_device_that_goes_missing_and_is_not_held_is_dropped() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface());
    catalog.refresh(&backend, None);

    assert_eq!(input_names(&catalog), vec!["webcam".to_owned()]);
}

#[test]
fn a_held_device_that_goes_missing_stays_listed() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface());
    catalog.refresh(&backend, Some(&held()));

    assert!(input_names(&catalog).contains(&"interface".to_owned()));
}

#[test]
fn a_held_device_that_goes_missing_keeps_the_channels_it_was_listed_with() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface());
    catalog.refresh(&backend, Some(&held()));

    let carried = catalog.hosts()[0]
        .inputs
        .iter()
        .find(|device| device.id == held().input)
        .expect("a held device stays listed");
    assert_eq!(carried.channels, vec![2, 4]);
}

#[test]
fn a_held_device_that_goes_missing_stays_listed_in_both_directions() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface());
    catalog.refresh(&backend, Some(&held()));

    assert_eq!(
        catalog.hosts()[0].outputs,
        vec![device("interface", vec![2])]
    );
}

#[test]
fn a_held_device_lost_in_one_direction_is_carried_into_that_one() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface_output());
    catalog.refresh(&backend, Some(&held()));

    assert_eq!(
        catalog.hosts()[0].outputs,
        vec![device("interface", vec![2])]
    );
}

#[test]
fn a_held_device_lost_in_one_direction_is_not_doubled_in_the_other() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface_output());
    catalog.refresh(&backend, Some(&held()));

    assert_eq!(
        input_names(&catalog),
        vec!["interface".to_owned(), "webcam".to_owned()]
    );
}

#[test]
fn carrying_into_a_host_that_is_still_listed_adds_no_second_one() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(without_the_interface_output());
    catalog.refresh(&backend, Some(&held()));

    assert_eq!(catalog.hosts().len(), 1);
}

#[test]
fn a_held_device_stays_listed_when_its_whole_host_goes_missing() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(Vec::new());
    catalog.refresh(&backend, Some(&held()));

    assert_eq!(catalog.hosts().len(), 1);
    assert_eq!(input_names(&catalog), vec!["interface".to_owned()]);
}

#[test]
fn a_held_device_that_was_never_listed_is_not_invented() {
    let backend = CountingBackend::new(without_the_interface());
    let mut catalog = DeviceCatalog::new(48_000);

    catalog.refresh(&backend, Some(&held()));

    assert!(!input_names(&catalog).contains(&"interface".to_owned()));
}

#[test]
fn a_held_device_is_carried_though_its_name_is_still_on_another() {
    let backend = CountingBackend::new(two_interfaces());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    backend.now_lists(one_host());
    catalog.refresh(&backend, Some(&holding_the_second_interface()));

    assert!(input_ids(&catalog).contains(&holding_the_second_interface().input));
}

#[test]
fn a_held_device_the_backend_still_lists_is_not_carried_beside_its_namesake() {
    let backend = CountingBackend::new(two_interfaces());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    catalog.refresh(&backend, Some(&held()));

    assert_eq!(catalog.hosts(), two_interfaces().as_slice());
}

#[test]
fn a_held_device_the_backend_still_lists_is_not_listed_twice() {
    let backend = CountingBackend::new(one_host());
    let mut catalog = DeviceCatalog::new(48_000);
    catalog.refresh(&backend, None);

    catalog.refresh(&backend, Some(&held()));

    assert_eq!(catalog.hosts(), one_host().as_slice());
}
