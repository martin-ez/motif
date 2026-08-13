//! One command queue, and the path it is dealt to.
//!
//! A queue has a single reader, and [`Commanded`] is it. The facts worth
//! stating are that everything waiting is dealt before the block it arrived in
//! is rendered, and that a path with a queue in front of it answers whatever it
//! answered without one, so a commanded path composes inside another.
//!
//! The path behind the queue is a stand-in that keeps what it was applied: what
//! reaches it is the subject, and a real path would only put its own rendering
//! between the assertion and the queue.

use motif::audio::{AudioPath, Command, Commanded, StreamConfig, command_channel};

const GAIN: f32 = 0.5;
const SAMPLE_RATE: u32 = 8;
const BLOCK: u32 = 4;

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: SAMPLE_RATE,
        block_size: BLOCK,
        input_channels: 1,
        output_channels: 1,
    }
}

/// A path that keeps the level it was last asked for and answers nothing else.
#[derive(Default)]
struct Levelled {
    gain: Option<f32>,
}

impl AudioPath for Levelled {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::SetGain(gain) => self.gain = Some(gain),
            _ => return false,
        }

        true
    }
}

#[test]
fn a_commanded_path_deals_what_arrived_before_the_block_it_arrived_in() {
    let (mut sender, receiver) = command_channel(4);
    let mut path = Commanded::new(receiver, Levelled::default());
    path.prepare(config());

    sender
        .send(Command::SetGain(GAIN))
        .expect("the queue has room for a test");
    path.render(&[1.0], &mut [0.0]);

    assert_eq!(path.path().gain, Some(GAIN));
}

#[test]
fn a_commanded_path_answers_for_the_path_it_holds() {
    let (_sender, receiver) = command_channel(4);
    let mut path = Commanded::new(receiver, Levelled::default());
    path.prepare(config());

    assert!(path.apply(Command::SetGain(GAIN)));
    assert!(!path.apply(Command::Undo));
    assert_eq!(path.path().gain, Some(GAIN));
}
