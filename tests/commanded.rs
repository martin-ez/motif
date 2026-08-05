//! One command queue, and more than one path behind it.
//!
//! A queue has a single reader, and what it deals to may be one path or a
//! composition of several. The facts worth stating are that every command
//! reaches the path it was meant for, that a path answering one is the end of
//! it, and that which path answers is what a composition says rather than
//! which of them renders first.
//!
//! Both real paths are used rather than stand-ins. The bug this states the
//! absence of appears only where an input monitor and a loop engine sit behind
//! one queue, each answering half of what arrives.

use motif::audio::{
    AudioPath, Command, CommandSender, Commanded, InputMonitor, StreamConfig, command_channel,
};
use motif::device::AudioProfile;
use motif::looper::{LoopEngine, PositionReader, Transport, position_meter, waveform_meter};

const GAIN: f32 = 0.5;
const UNITY: f32 = 1.0;
const SAMPLE_RATE: u32 = 8;
const BLOCK: u32 = 4;

/// Which of two paths goes first.
#[derive(Clone, Copy)]
enum First {
    Monitor,
    Engine,
}

/// The monitor and the engine behind one queue.
///
/// It answers nothing of its own: a command goes to whichever path is offered
/// it first and takes it. Rendering has an order of its own, so a test can vary
/// one and hold the other.
struct Both {
    monitor: InputMonitor,
    engine: LoopEngine,
    answers: First,
    renders: First,
}

impl AudioPath for Both {
    fn prepare(&mut self, config: StreamConfig) {
        self.monitor.prepare(config);
        self.engine.prepare(config);
    }

    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        match self.renders {
            First::Monitor => {
                self.monitor.render(captured, playing);
                self.engine.render(captured, playing);
            }
            First::Engine => {
                self.engine.render(captured, playing);
                self.monitor.render(captured, playing);
            }
        }
    }

    fn apply(&mut self, command: Command) -> bool {
        match self.answers {
            First::Monitor => self.monitor.apply(command) || self.engine.apply(command),
            First::Engine => self.engine.apply(command) || self.monitor.apply(command),
        }
    }
}

fn eight_frame_profile() -> AudioProfile {
    AudioProfile {
        sample_rate: SAMPLE_RATE,
        block_size: BLOCK,
        max_loop_seconds: 1,
    }
}

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: SAMPLE_RATE,
        block_size: BLOCK,
        input_channels: 1,
        output_channels: 1,
    }
}

/// Both paths on one queue, answering in one order and rendering in the other
/// the test asks for.
fn both(answers: First, renders: First) -> (Commanded<Both>, CommandSender, PositionReader) {
    let (sender, receiver) = command_channel(8);
    let (writer, position) = position_meter();
    let mut path = Commanded::new(
        receiver,
        Both {
            monitor: InputMonitor::new(),
            engine: LoopEngine::new(eight_frame_profile(), writer, waveform_meter().0),
            answers,
            renders,
        },
    );
    path.prepare(config());

    (path, sender, position)
}

/// Queue `command`, as the application thread does.
fn press(sender: &mut CommandSender, command: Command) {
    sender.send(command).expect("the queue has room for a test");
}

/// Render one block of `captured` and return what the composition played.
fn played(path: &mut Commanded<Both>, captured: &[f32]) -> Vec<f32> {
    let mut playing = vec![0.0; captured.len()];
    path.render(captured, &mut playing);

    playing
}

/// The level the monitor inside the composition is playing the input at.
fn monitored(path: &Commanded<Both>) -> f32 {
    path.path().monitor.gain().target()
}

#[test]
fn a_command_for_each_path_reaches_both_of_them() {
    let (mut path, mut sender, position) = both(First::Monitor, First::Monitor);

    press(&mut sender, Command::SetGain(GAIN));
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut path, &[1.0, 1.0]);

    assert_eq!(monitored(&path), GAIN);
    assert_eq!(position.read().recorded(), 2);
}

#[test]
fn which_path_answers_does_not_change_with_the_render_order() {
    let (mut path, mut sender, position) = both(First::Monitor, First::Engine);

    press(&mut sender, Command::SetGain(GAIN));
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut path, &[1.0, 1.0]);

    assert_eq!(monitored(&path), GAIN);
    assert_eq!(position.read().recorded(), 2);
}

#[test]
fn a_command_the_monitor_answered_does_not_reach_the_engine() {
    let (mut path, mut sender, _position) = both(First::Monitor, First::Monitor);

    press(&mut sender, Command::SetGain(GAIN));
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut path, &[1.0, 1.0]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(played(&mut path, &[0.0, 0.0]), [1.0, 1.0]);
}

#[test]
fn the_path_offered_a_command_first_is_the_one_that_owns_it() {
    let (mut path, mut sender, _position) = both(First::Engine, First::Monitor);

    press(&mut sender, Command::SetGain(GAIN));
    press(&mut sender, Command::SetTransport(Transport::Recording));
    played(&mut path, &[1.0, 1.0]);
    press(&mut sender, Command::SetTransport(Transport::Playing));

    assert_eq!(monitored(&path), UNITY);
    assert_eq!(played(&mut path, &[0.0, 0.0]), [GAIN, GAIN]);
}

#[test]
fn a_commanded_path_deals_what_arrived_before_the_block_it_arrived_in() {
    let (mut sender, receiver) = command_channel(4);
    let mut path = Commanded::new(receiver, InputMonitor::new());
    path.prepare(config());

    press(&mut sender, Command::SetGain(GAIN));
    path.render(&[1.0], &mut [0.0]);

    assert_eq!(path.path().gain().target(), GAIN);
}

#[test]
fn a_commanded_path_answers_for_the_path_it_holds() {
    let (_sender, receiver) = command_channel(4);
    let mut path = Commanded::new(receiver, InputMonitor::new());
    path.prepare(config());

    assert!(path.apply(Command::SetGain(GAIN)));
    assert!(!path.apply(Command::Undo));
    assert_eq!(path.path().gain().target(), GAIN);
}
