//! The owner of the loop, on the thread that moves it along.
//!
//! [`LoopEngine`] holds the buffer, the transport, the commands and the
//! playhead together: a block of input goes in, a block of output comes out,
//! and what the player asked for in between arrived over the command queue. It
//! belongs on the playback side, downstream of the ring the capture callback
//! writes into, which gives the buffer one owner on one thread and keeps the
//! playhead on the thread that moves it.
//!
//! Invariant 2 shapes the block: the loop and the scratch the input is gained
//! into are both allocated in setup, and a block is a fixed number of passes
//! over buffers that are already there.

use crate::audio::{AudioPath, Command, CommandReceiver, StreamConfig};
use crate::device::AudioProfile;

use super::{LoopBuffer, LoopPosition, PositionWriter, Transport};

const UNITY_GAIN: f32 = 1.0;

/// The loop, and what a block of input makes of it.
///
/// Record captures the input, overdub layers over what is there, and play runs
/// the loop under the live input; a player reaches all three over the command
/// queue. Gain scales the input ahead of all of it, and mute silences the
/// output alone — a muted take is still recorded.
///
/// A layer is recorded after the loop is played, so the input is heard once
/// rather than twice. It appends from the layer's start rather than from the
/// playhead, and one the stack has no room for takes nothing.
///
/// ```
/// use motif::audio::{AudioPath, Command, SendError, command_channel};
/// use motif::device::DeviceProfile;
/// use motif::looper::{LoopEngine, Transport, position_meter};
///
/// let (mut player, commands) = command_channel(4);
/// let (writer, position) = position_meter();
/// let mut engine = LoopEngine::new(DeviceProfile::TARGET.audio, commands, writer);
///
/// player.send(Command::SetTransport(Transport::Recording))?;
/// engine.render(&[0.25, 0.5], &mut [0.0; 2]);
///
/// player.send(Command::SetTransport(Transport::Playing))?;
/// let mut heard = [0.0; 2];
/// engine.render(&[0.0; 2], &mut heard);
///
/// assert_eq!(heard, [0.25, 0.5]);
/// assert_eq!(position.read().recorded(), 2);
/// # Ok::<(), SendError>(())
/// ```
pub struct LoopEngine {
    buffer: LoopBuffer,
    commands: CommandReceiver,
    position: PositionWriter,
    gained: Box<[f32]>,
    transport: Transport,
    playhead: usize,
    layer_open: bool,
    gain: f32,
    muted: bool,
}

impl LoopEngine {
    /// An engine over the longest loop `profile` allows, idle and unmuted at
    /// unity gain, ordered by `commands` and publishing to `position`.
    ///
    /// Its buffers are allocated here and never again, so this belongs in
    /// setup, before the stream starts.
    ///
    /// # Panics
    ///
    /// Panics on a profile with no loop to record or no block to record it in,
    /// either being a mistake in setup rather than a condition worth reporting
    /// from the real-time thread.
    pub fn new(profile: AudioProfile, commands: CommandReceiver, position: PositionWriter) -> Self {
        let block = profile.block_size as usize;
        assert!(block > 0, "an engine renders nothing block by block");

        Self {
            buffer: LoopBuffer::for_profile(profile),
            commands,
            position,
            gained: vec![0.0; block].into_boxed_slice(),
            transport: Transport::default(),
            playhead: 0,
            layer_open: true,
            gain: UNITY_GAIN,
            muted: false,
        }
    }

    fn apply_arrivals(&mut self) {
        for _ in 0..self.commands.pending() {
            if let Some(command) = self.commands.recv() {
                self.apply(command);
            }
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::SetTransport(transport) => self.move_to(transport),
            Command::SetMuted(muted) => self.muted = muted,
            Command::SetGain(gain) => self.gain = gain,
            Command::Undo => {
                if self.buffer.undo() {
                    self.layer_open = false;
                }
            }
            Command::Clear => {
                self.buffer.clear();
                self.layer_open = true;
                self.playhead = 0;
            }
        }
    }

    fn move_to(&mut self, transport: Transport) {
        if transport == Transport::Overdubbing && self.transport != Transport::Overdubbing {
            self.layer_open = self.buffer.depth() < LoopBuffer::LAYERS;
            if self.layer_open {
                self.buffer.overdub();
            }
        }
        if transport.plays_loop() && !self.transport.plays_loop() {
            self.playhead = 0;
        }

        self.transport = transport;
    }

    fn mix_block(&mut self, captured: &[f32], playing: &mut [f32]) {
        let frames = captured.len();
        for (level, sample) in self.gained[..frames].iter_mut().zip(captured) {
            *level = sample * self.gain;
        }
        let gained = &self.gained[..frames];

        if self.transport.plays_loop() {
            self.playhead = self.buffer.play_into(playing, self.playhead);
        }
        if self.transport.captures_input() && self.layer_open {
            self.buffer.record(gained);
            if !self.transport.plays_loop() {
                self.playhead = self.buffer.len();
            }
        }

        for (played, level) in playing.iter_mut().zip(gained) {
            *played += level;
        }
        if self.muted {
            playing.fill(0.0);
        }
    }

    fn publish(&mut self) {
        self.position.publish(LoopPosition::new(
            frame_count(self.playhead),
            frame_count(self.buffer.len()),
        ));
    }
}

fn frame_count(frames: usize) -> u32 {
    u32::try_from(frames).unwrap_or(u32::MAX)
}

impl AudioPath for LoopEngine {
    /// Nothing to prepare: the loop and the scratch are sized from the profile
    /// the engine was built with, and a longer block is worked in chunks.
    fn prepare(&mut self, _config: StreamConfig) {}

    /// Applies what the player asked for, plays the loop under their input, and
    /// publishes the playhead the block ended on.
    ///
    /// Two lengths become the shorter of them, and a block longer than the
    /// scratch is worked in chunks: neither is a panic on the audio thread.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.apply_arrivals();

        let frames = captured.len().min(playing.len());
        let chunk = self.gained.len();
        let chunks = captured[..frames]
            .chunks(chunk)
            .zip(playing[..frames].chunks_mut(chunk));
        for (captured, playing) in chunks {
            self.mix_block(captured, playing);
        }

        self.publish();
    }
}
