//! The level a stream opens at, and the ramp it reaches it over.
//!
//! Two things a stream owes the room it plays into, stated where they are true:
//! a path is prepared once per stream opened, so what this file calls preparing
//! again is a device coming back after a fault, and what it calls the first
//! frame is the first frame a player would have heard.
//!
//! Levels are read off a path playing a fixed value, where the played samples
//! are the trim itself, and a block long enough to outlast the ramp is what a
//! settled level is read from.

use std::sync::{Arc, Mutex};

use motif::audio::{AudioPath, Command, GUARDED_LEVEL, Gain, Opening, StreamConfig};

const SAMPLE_RATE: u32 = 48_000;
const RAMP_FRAMES: usize = SAMPLE_RATE as usize * Gain::RAMP / 1_000;
const SETTLED: usize = RAMP_FRAMES * 2;
const UNITY: f32 = 1.0;
const HALF: f32 = 0.5;
const TOLERANCE: f32 = 1e-6;
const GUARDED_DECIBELS: f32 = -12.0;
const DECIBELS_PER_DECADE: f32 = 20.0;

fn config() -> StreamConfig {
    StreamConfig {
        sample_rate: SAMPLE_RATE,
        block_size: 256,
        input_channels: 2,
        output_channels: 2,
    }
}

/// A path playing one value, which remembers what it was told.
#[derive(Clone)]
struct Noted {
    plays: f32,
    answers: bool,
    prepared: Arc<Mutex<Vec<StreamConfig>>>,
    commands: Arc<Mutex<Vec<Command>>>,
}

impl Noted {
    fn playing(plays: f32) -> Self {
        Self {
            plays,
            answers: true,
            prepared: Arc::new(Mutex::new(Vec::new())),
            commands: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn refusing() -> Self {
        Self {
            answers: false,
            ..Self::playing(UNITY)
        }
    }

    fn prepared_with(&self) -> Vec<StreamConfig> {
        self.prepared.lock().expect("no thread holds it").clone()
    }

    fn offered(&self) -> Vec<Command> {
        self.commands.lock().expect("no thread holds it").clone()
    }
}

impl AudioPath for Noted {
    fn prepare(&mut self, config: StreamConfig) {
        self.prepared
            .lock()
            .expect("no thread holds it")
            .push(config);
    }

    fn render(&mut self, _captured: &[f32], playing: &mut [f32]) {
        playing.fill(self.plays);
    }

    fn apply(&mut self, command: Command) -> bool {
        self.commands
            .lock()
            .expect("no thread holds it")
            .push(command);

        self.answers
    }
}

fn opened(level: f32, plays: f32) -> Opening<Noted> {
    let mut path = Opening::at(level, Noted::playing(plays));
    path.prepare(config());

    path
}

fn played(path: &mut Opening<Noted>, frames: usize) -> Vec<f32> {
    let captured = vec![UNITY; frames];
    let mut playing = vec![0.0; frames];
    path.render(&captured, &mut playing);

    playing
}

fn settled(path: &mut Opening<Noted>) -> f32 {
    let block = played(path, SETTLED);

    block[block.len() - 1]
}

fn largest_step(block: &[f32]) -> f32 {
    block
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_stream_that_has_just_opened_plays_silence_in_its_first_frame() {
    let mut path = opened(UNITY, UNITY);

    assert_eq!(played(&mut path, 1)[0], 0.0);
}

#[test]
fn an_opening_stream_does_not_reach_its_level_in_one_step() {
    let mut path = opened(UNITY, UNITY);

    let block = played(&mut path, RAMP_FRAMES);

    assert!(largest_step(&block) < UNITY / RAMP_FRAMES as f32 + TOLERANCE);
}

#[test]
fn an_opening_stream_reaches_unity_after_the_ramp() {
    let mut path = opened(UNITY, UNITY);

    assert!((settled(&mut path) - UNITY).abs() < TOLERANCE);
}

#[test]
fn an_opening_stream_settles_at_the_level_it_was_given() {
    let mut path = opened(GUARDED_LEVEL, UNITY);

    assert!((settled(&mut path) - GUARDED_LEVEL).abs() < TOLERANCE);
}

#[test]
fn a_stream_reopened_after_a_device_fault_comes_up_from_silence_again() {
    let mut path = opened(UNITY, UNITY);
    settled(&mut path);

    path.prepare(config());

    assert_eq!(played(&mut path, 1)[0], 0.0);
}

#[test]
fn a_stream_reopened_after_a_device_fault_reaches_its_level_over_the_ramp() {
    let mut path = opened(UNITY, UNITY);
    settled(&mut path);

    path.prepare(config());

    assert!(largest_step(&played(&mut path, RAMP_FRAMES)) < UNITY / RAMP_FRAMES as f32 + TOLERANCE);
}

#[test]
fn a_stream_that_was_never_prepared_arrives_at_once() {
    let mut path = Opening::at(UNITY, Noted::playing(UNITY));

    let block = played(&mut path, 2);

    assert_eq!(block[0], 0.0);
    assert!((block[1] - UNITY).abs() < TOLERANCE);
}

#[test]
fn the_guarded_level_is_twelve_decibels_below_unity() {
    let twelve_decibels_down = 10.0_f32.powf(GUARDED_DECIBELS / DECIBELS_PER_DECADE);

    assert!((GUARDED_LEVEL - twelve_decibels_down).abs() < TOLERANCE);
}

#[test]
fn the_guarded_level_leaves_unity_at_the_top_of_the_encoders_range() {
    assert!((GUARDED_LEVEL * Gain::CEILING - UNITY).abs() < TOLERANCE);
}

#[test]
fn an_opening_stream_plays_what_the_path_under_it_plays() {
    let mut path = opened(UNITY, HALF);

    assert!((settled(&mut path) - HALF).abs() < TOLERANCE);
}

#[test]
fn an_opening_stream_trims_what_it_plays_rather_than_what_it_captured() {
    let mut path = opened(GUARDED_LEVEL, UNITY);
    let mut playing = vec![0.0; SETTLED];

    path.render(&[], &mut playing);

    assert!((playing[playing.len() - 1] - GUARDED_LEVEL).abs() < TOLERANCE);
}

#[test]
fn an_opening_stream_prepares_the_path_under_it() {
    let under = Noted::playing(UNITY);
    let mut path = Opening::at(UNITY, under.clone());

    path.prepare(config());

    assert_eq!(under.prepared_with(), [config()]);
}

#[test]
fn an_opening_stream_offers_a_command_to_the_path_under_it() {
    let under = Noted::playing(UNITY);
    let mut path = Opening::at(UNITY, under.clone());

    assert!(path.apply(Command::SetGain(HALF)));
    assert_eq!(under.offered(), [Command::SetGain(HALF)]);
}

#[test]
fn an_opening_stream_answers_nothing_the_path_under_it_refuses() {
    let mut path = Opening::at(UNITY, Noted::refusing());

    assert!(!path.apply(Command::Undo));
}
