//! A path that outlives the stream it was playing on.
//!
//! A run has one loop engine and opens a stream more than once — a device is
//! chosen, a device is refused, a device comes back — so what a stream plays
//! has to survive being handed to one. The facts worth stating are that a
//! second stream plays the path the first one had rather than silence, that a
//! path a backend refused to take is back where it came from, and that a path
//! already out on loan is not lent twice.

use std::sync::{Arc, Mutex};

use motif::audio::{
    AudioBackend, AudioPath, Command, DeviceId, DeviceLink, DeviceSelection, Escrow, NullBackend,
    NullStream, StreamConfig, StreamRequest,
};

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

fn no_such_device() -> DeviceSelection {
    DeviceSelection {
        input: DeviceId::named("nothing of the sort"),
        ..selection()
    }
}

/// How many streams a path has served, published where a test can read it and
/// played so that a stream's output says which serving it is on.
///
/// A builder handing out a fresh path counts one every time; the one path lent
/// again counts up; a stream given no path at all counts nothing. It answers
/// what clears a loop and nothing else, so that a loan can be asked whether it
/// passes a command on.
#[derive(Clone, Default)]
struct Served {
    servings: Arc<Mutex<Vec<usize>>>,
}

impl Served {
    fn counted(&self) -> Vec<usize> {
        self.servings
            .lock()
            .expect("no test holds this across a panic")
            .clone()
    }

    fn path(&self) -> ServingPath {
        ServingPath {
            served: 0,
            servings: Arc::clone(&self.servings),
        }
    }
}

struct ServingPath {
    served: usize,
    servings: Arc<Mutex<Vec<usize>>>,
}

impl AudioPath for ServingPath {
    fn prepare(&mut self, _config: StreamConfig) {
        self.served += 1;
        self.servings
            .lock()
            .expect("no test holds this across a panic")
            .push(self.served);
    }

    fn render(&mut self, _captured: &[f32], playing: &mut [f32]) {
        playing.fill(self.served as f32);
    }

    fn apply(&mut self, command: Command) -> bool {
        matches!(command, Command::Clear)
    }
}

fn opened(escrow: &Escrow<ServingPath>) -> NullStream {
    NullBackend::rounding(granted())
        .open(&selection(), request(), escrow.lend())
        .expect("null backend opens")
}

fn played(stream: &mut NullStream) -> [f32; 4] {
    let mut playing = [0.0; 4];
    stream.block(&[1.0; 4], &mut playing);
    playing
}

#[test]
fn a_lent_path_plays_what_it_was_lent_from() {
    let escrow = Escrow::holding(Served::default().path());

    let mut stream = opened(&escrow);

    assert_eq!(played(&mut stream), [1.0; 4]);
}

#[test]
fn a_second_stream_plays_the_path_the_first_one_had() {
    let escrow = Escrow::holding(Served::default().path());

    let first = opened(&escrow);
    drop(first);
    let mut second = opened(&escrow);

    assert_eq!(played(&mut second), [2.0; 4]);
}

#[test]
fn a_path_still_on_loan_leaves_a_second_stream_silent() {
    let escrow = Escrow::holding(Served::default().path());

    let _first = opened(&escrow);
    let mut second = opened(&escrow);

    assert_eq!(played(&mut second), [0.0; 4]);
}

#[test]
fn a_loan_answers_whatever_the_path_it_holds_answers() {
    let escrow = Escrow::holding(Served::default().path());

    let mut lent = escrow.lend();

    assert!(lent.apply(Command::Clear));
    assert!(!lent.apply(Command::Undo));
}

#[test]
fn a_loan_holding_nothing_answers_nothing() {
    let escrow = Escrow::holding(Served::default().path());

    let _out = escrow.lend();
    let mut holding_nothing = escrow.lend();

    assert!(!holding_nothing.apply(Command::Clear));
}

#[test]
fn a_loan_that_held_nothing_takes_nothing_home() {
    let escrow = Escrow::holding(Served::default().path());

    let first = opened(&escrow);
    let held_nothing = opened(&escrow);
    drop(first);
    drop(held_nothing);
    let mut third = opened(&escrow);

    assert_eq!(played(&mut third), [2.0; 4]);
}

#[test]
fn a_path_a_backend_refused_is_back_where_it_came_from() {
    let escrow = Escrow::holding(Served::default().path());
    let backend = NullBackend::rounding(granted());

    let refused = backend.open(&no_such_device(), request(), escrow.lend());
    let mut opened = backend
        .open(&selection(), request(), escrow.lend())
        .expect("null backend opens");

    assert!(refused.is_err());
    assert_eq!(played(&mut opened), [1.0; 4]);
}

#[test]
fn reopening_a_link_hands_the_new_stream_the_same_path() {
    let served = Served::default();
    let escrow = Escrow::holding(served.path());
    let mut link = DeviceLink::new(
        NullBackend::rounding(granted()),
        request(),
        selection(),
        move || escrow.lend(),
    );

    link.open();
    link.settled();
    link.open();
    link.settled();

    assert_eq!(served.counted(), vec![1, 2]);
}

#[test]
fn selecting_another_device_keeps_the_path_that_was_playing() {
    let served = Served::default();
    let escrow = Escrow::holding(served.path());
    let mut link = DeviceLink::new(
        NullBackend::rounding(granted()),
        request(),
        selection(),
        move || escrow.lend(),
    );

    link.open();
    link.settled();
    link.select(selection());
    link.settled();

    assert_eq!(served.counted(), vec![1, 2]);
}
