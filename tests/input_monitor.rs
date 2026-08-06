//! The path that plays the input at a level the player controls.
//!
//! Driven the way the callback drives it: commands are queued from this thread,
//! and the queue in front of the path deals them when it renders a block.
//! Nothing here opens a device — what a path does with a block is answerable
//! without one.
//!
//! Levels are read off a block of ones, where the played samples are the gain
//! itself, and a block long enough to outlast the ramp is what a settled level
//! is read from.

use motif::audio::{
    AudioPath, Command, CommandSender, Commanded, Gain, InputMonitor, StreamConfig,
};
use motif::looper::Transport;

const SAMPLE_RATE: u32 = 48_000;
const RAMP_FRAMES: usize = SAMPLE_RATE as usize * Gain::RAMP / 1_000;
const SETTLED: usize = RAMP_FRAMES * 2;
const HALF: f32 = 0.5;
const TOLERANCE: f32 = 1e-6;
const FAR_ABOVE_THE_CEILING: f32 = 1_000.0;

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: SAMPLE_RATE,
        block_size: 256,
        input_channels: 2,
        output_channels: 2,
    }
}

fn monitoring() -> (CommandSender, Commanded<InputMonitor>) {
    let (sender, receiver) = motif::audio::command_channel(16);
    let mut path = Commanded::new(receiver, InputMonitor::new());
    path.prepare(config());

    (sender, path)
}

fn played(path: &mut Commanded<InputMonitor>, frames: usize) -> Vec<f32> {
    let captured = vec![1.0; frames];
    let mut playing = vec![0.0; frames];
    path.render(&captured, &mut playing);

    playing
}

fn settled(path: &mut Commanded<InputMonitor>) -> f32 {
    let block = played(path, SETTLED);

    block[block.len() - 1]
}

fn sent(sender: &mut CommandSender, command: Command) {
    sender.send(command).expect("the queue has room");
}

#[test]
fn a_monitor_with_no_commands_plays_what_it_captured() {
    let (_sender, mut path) = monitoring();

    assert_eq!(played(&mut path, 4), [1.0; 4]);
}

#[test]
fn a_monitor_plays_nothing_it_was_not_given() {
    let (_sender, mut path) = monitoring();
    let mut playing = vec![0.0; 4];

    path.render(&[], &mut playing);

    assert_eq!(playing, [0.0; 4]);
}

#[test]
fn a_gain_command_sets_the_level_that_is_played() {
    let (mut sender, mut path) = monitoring();

    sent(&mut sender, Command::SetGain(HALF));

    assert!((settled(&mut path) - HALF).abs() < TOLERANCE);
}

#[test]
fn a_gain_command_above_the_ceiling_plays_at_the_ceiling() {
    let (mut sender, mut path) = monitoring();

    sent(&mut sender, Command::SetGain(FAR_ABOVE_THE_CEILING));

    assert!((settled(&mut path) - Gain::CEILING).abs() < TOLERANCE);
}

#[test]
fn a_gain_command_is_acted_on_in_the_block_it_arrived_in() {
    let (mut sender, mut path) = monitoring();
    sent(&mut sender, Command::SetGain(0.0));

    let block = played(&mut path, 8);

    assert!(block[7] < block[0]);
}

#[test]
fn a_mute_command_silences_the_output() {
    let (mut sender, mut path) = monitoring();

    sent(&mut sender, Command::SetMuted(true));

    assert!(settled(&mut path).abs() < TOLERANCE);
}

#[test]
fn unmuting_brings_the_level_back() {
    let (mut sender, mut path) = monitoring();
    sent(&mut sender, Command::SetGain(HALF));
    sent(&mut sender, Command::SetMuted(true));
    settled(&mut path);

    sent(&mut sender, Command::SetMuted(false));

    assert!((settled(&mut path) - HALF).abs() < TOLERANCE);
}

#[test]
fn a_muted_monitor_does_not_cut_the_output_dead() {
    let (mut sender, mut path) = monitoring();

    sent(&mut sender, Command::SetMuted(true));
    let block = played(&mut path, RAMP_FRAMES + 1);

    let largest = block
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);

    assert!(largest < 1.0 / RAMP_FRAMES as f32 + TOLERANCE);
}

#[test]
fn a_command_the_monitor_does_not_answer_leaves_the_level_alone() {
    let (mut sender, mut path) = monitoring();
    sent(&mut sender, Command::SetGain(HALF));
    settled(&mut path);

    sent(&mut sender, Command::Clear);
    sent(&mut sender, Command::Undo);

    assert!((settled(&mut path) - HALF).abs() < TOLERANCE);
}

#[test]
fn every_command_waiting_is_taken_before_the_block_is_played() {
    let (mut sender, mut path) = monitoring();

    sent(&mut sender, Command::SetGain(0.25));
    sent(&mut sender, Command::SetGain(HALF));

    assert!((settled(&mut path) - HALF).abs() < TOLERANCE);
}

#[test]
fn a_monitor_answers_what_moves_the_level_and_nothing_else() {
    let mut path = InputMonitor::new();

    assert!(path.apply(Command::SetGain(HALF)));
    assert!(path.apply(Command::SetMuted(true)));
    assert!(!path.apply(Command::Undo));
    assert!(!path.apply(Command::Clear));
    assert!(!path.apply(Command::SetTransport(Transport::Recording)));
}

#[test]
fn the_ramp_is_in_the_frames_the_device_granted() {
    let (sender, receiver) = motif::audio::command_channel(16);
    let mut sender = sender;
    let mut path = Commanded::new(receiver, InputMonitor::new());
    path.prepare(StreamConfig {
        sample_rate: SAMPLE_RATE * 2,
        ..config()
    });

    sent(&mut sender, Command::SetGain(0.0));
    let block = played(&mut path, RAMP_FRAMES + 1);

    assert!(block[block.len() - 1] > 0.0);
}
