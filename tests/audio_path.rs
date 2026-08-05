//! The seam a caller puts work on the audio thread through.
//!
//! Exercised against a backend with no hardware behind it, so that it runs
//! where no audio device exists. What a stream plays is the path's decision
//! rather than the stream's, so the facts worth stating are that a path is
//! handed the frames that were captured, that what it writes is what comes out,
//! that a path with nothing to play leaves silence, and that it learns the
//! configuration the device granted rather than the one that was asked for.

use std::sync::{Arc, Mutex};

use motif::audio::{
    AudioBackend, AudioPath, Command, DeviceSelection, InputMonitor, NullBackend, Passthrough,
    StreamConfig, StreamRequest,
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

/// A path that plays one value, whatever it was handed.
struct Tone(f32);

impl AudioPath for Tone {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], playing: &mut [f32]) {
        playing.fill(self.0);
    }

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// A path that plays nothing at all.
struct Silence;

impl AudioPath for Silence {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, _captured: &[f32], _playing: &mut [f32]) {}

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// A path written to the contract and nothing softer: a frame out for every
/// frame in, which panics on anything that hands it two lengths.
struct FrameForFrame;

impl AudioPath for FrameForFrame {
    fn prepare(&mut self, _config: StreamConfig) {}

    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        playing.copy_from_slice(captured);
    }

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

/// A path that keeps what it was told and what it was handed, so that a test
/// can read them back after it has been moved into a stream.
#[derive(Clone, Default)]
struct Heard {
    config: Arc<Mutex<Option<StreamConfig>>>,
    captured: Arc<Mutex<Vec<f32>>>,
}

impl Heard {
    fn config(&self) -> Option<StreamConfig> {
        *self
            .config
            .lock()
            .expect("no test holds this across a panic")
    }

    fn captured(&self) -> Vec<f32> {
        self.captured
            .lock()
            .expect("no test holds this across a panic")
            .clone()
    }
}

impl AudioPath for Heard {
    fn prepare(&mut self, config: StreamConfig) {
        *self
            .config
            .lock()
            .expect("no test holds this across a panic") = Some(config);
    }

    fn render(&mut self, captured: &[f32], _playing: &mut [f32]) {
        self.captured
            .lock()
            .expect("no test holds this across a panic")
            .extend_from_slice(captured);
    }

    fn apply(&mut self, _command: Command) -> bool {
        false
    }
}

#[test]
fn passthrough_plays_the_frames_it_was_handed() {
    let mut path = Passthrough::new();
    let mut played = [0.0; 3];

    path.render(&[0.25, 0.5, 0.75], &mut played);

    assert_eq!(played, [0.25, 0.5, 0.75]);
}

#[test]
fn passthrough_answers_no_command_at_all() {
    let mut path = Passthrough::new();

    assert!(!path.apply(Command::SetGain(0.5)));
    assert!(!path.apply(Command::SetMuted(true)));
    assert!(!path.apply(Command::Clear));
}

#[test]
fn a_path_decides_what_a_stream_plays() {
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), Tone(0.5))
        .expect("null backend opens");
    let mut played = [0.0; 4];

    stream.block(&[1.0; 4], &mut played);

    assert_eq!(played, [0.5; 4]);
}

#[test]
fn a_path_hears_the_frames_the_stream_captured() {
    let heard = Heard::default();
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), heard.clone())
        .expect("null backend opens");

    stream.block(&[0.1, 0.2], &mut [0.0; 2]);

    assert_eq!(heard.captured(), vec![0.1, 0.2]);
}

#[test]
fn a_path_that_plays_nothing_leaves_silence() {
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), Silence)
        .expect("null backend opens");
    let mut played = [9.0; 4];

    stream.block(&[1.0; 4], &mut played);

    assert_eq!(played, [0.0; 4]);
}

#[test]
fn a_path_is_prepared_with_what_the_device_granted() {
    let heard = Heard::default();
    let asked_for_a_shorter_block = StreamRequest {
        sample_rate: 48_000,
        block_size: 128,
    };

    let _stream = NullBackend::rounding(granted())
        .open(&selection(), asked_for_a_shorter_block, heard.clone())
        .expect("a rounding device grants its own configuration");

    assert_eq!(heard.config(), Some(granted()));
}

#[test]
fn a_path_is_prepared_before_it_plays_anything() {
    let heard = Heard::default();
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), heard.clone())
        .expect("null backend opens");

    stream.block(&[0.5], &mut [0.0; 1]);

    assert_eq!(heard.config(), Some(granted()));
    assert_eq!(heard.captured(), vec![0.5]);
}

#[test]
fn a_stand_in_callback_hands_the_path_a_frame_for_a_frame() {
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), FrameForFrame)
        .expect("null backend opens");
    let mut played = [9.0; 2];

    stream.block(&[1.0; 4], &mut played);

    assert_eq!(played, [1.0, 1.0]);
}

#[test]
fn a_path_that_is_there_plays_what_it_would_have_played() {
    let mut path = Some(Tone(0.5));
    let mut played = [0.0; 3];

    path.render(&[1.0; 3], &mut played);

    assert_eq!(played, [0.5; 3]);
}

#[test]
fn a_path_that_was_taken_plays_nothing() {
    let mut taken: Option<Tone> = None;
    let mut played = [0.0; 3];

    taken.render(&[1.0; 3], &mut played);

    assert_eq!(played, [0.0; 3]);
}

#[test]
fn a_path_that_is_there_is_prepared_with_the_configuration() {
    let heard = Heard::default();
    let mut path = Some(heard.clone());

    path.prepare(granted());

    assert_eq!(heard.config(), Some(granted()));
}

#[test]
fn a_path_that_was_taken_prepares_nothing() {
    let mut taken: Option<Heard> = None;

    taken.prepare(granted());

    assert!(taken.is_none());
}

#[test]
fn a_path_that_is_there_answers_what_it_would_have_answered() {
    let mut path = Some(InputMonitor::new());

    assert!(path.apply(Command::SetGain(0.5)));
    assert!(!path.apply(Command::Clear));
}

#[test]
fn a_path_that_was_taken_answers_nothing() {
    let mut taken: Option<InputMonitor> = None;

    assert!(!taken.apply(Command::SetGain(0.5)));
}

#[test]
fn a_path_handed_over_once_is_gone_the_second_time() {
    let mut engine = Some(Tone(0.5));
    let mut build = move || engine.take();

    assert!(build().is_some());
    assert!(build().is_none());
}

#[test]
fn a_stand_in_callback_silences_what_the_path_was_not_given() {
    let mut stream = NullBackend::rounding(granted())
        .open(&selection(), request(), FrameForFrame)
        .expect("null backend opens");
    let mut played = [9.0; 4];

    stream.block(&[1.0, 1.0], &mut played);

    assert_eq!(played, [1.0, 1.0, 0.0, 0.0]);
}
