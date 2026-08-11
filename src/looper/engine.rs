//! The owner of the loop, on the thread that moves it along.
//!
//! [`LoopEngine`] holds the buffer, the transport and the playhead together: a
//! block of input goes in, a block of output comes out, and what the player
//! asked for in between arrived as commands a
//! [`Commanded`](crate::audio::Commanded) dealt it. It belongs on the playback
//! side, downstream of the ring the capture callback writes into, which gives
//! the buffer one owner on one thread and keeps the playhead on the thread that
//! moves it.
//!
//! Invariant 2 shapes the block: the loop and the scratch the input is gained
//! into are both allocated in setup, and a block is a fixed number of passes
//! over buffers that are already there.

use crate::audio::{AudioPath, Command, Gain, StreamConfig, held};
use crate::device::AudioProfile;
use crate::seq::Bars;

use super::{LoopBuffer, LoopPosition, PositionWriter, TakeWriter, Transport, WaveformWriter};

/// The loop, and what a block of input makes of it.
///
/// Record captures the input, overdub layers over what is there, and play runs
/// the loop under the live input; a player reaches all three over the command
/// queue. A [`Gain`] ramps the input ahead of all of it, and a second one over
/// the mixed output carries the mute, so a muted take is still recorded.
///
/// A layer is recorded after the loop is played, so the input is heard once
/// rather than twice. It is written from the playhead it was punched in at and
/// carries on round the boundary; one the stack has no room for takes nothing.
///
/// ```
/// use motif::audio::{AudioPath, Command, Commanded, SendError, command_channel};
/// use motif::device::DeviceProfile;
/// use motif::looper::{LoopEngine, Transport, position_meter, take_handoff, waveform_meter};
///
/// let (mut player, commands) = command_channel(4);
/// let (writer, position) = position_meter();
/// let (shape, _drawn) = waveform_meter();
/// let (crossing, _takes) = take_handoff(DeviceProfile::TARGET.audio);
/// let engine = LoopEngine::new(DeviceProfile::TARGET.audio, writer, shape, crossing);
/// let mut engine = Commanded::new(commands, engine);
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
    position: PositionWriter,
    waveform: WaveformWriter,
    takes: TakeWriter,
    gained: Box<[f32]>,
    transport: Transport,
    playhead: usize,
    layer_open: bool,
    gain: Gain,
    output: Gain,
    bars: Option<Bars>,
}

impl LoopEngine {
    /// An engine over the longest loop `profile` allows, idle and unmuted at
    /// unity gain, publishing where the loop is to `position`, what is in it to
    /// `waveform`, and each finished take to `takes`. Its buffers are allocated
    /// here and never again, so this belongs in setup, before the stream starts.
    ///
    /// # Panics
    ///
    /// Panics on a profile with no loop to record or no block to record it in,
    /// either being a mistake in setup rather than a condition worth reporting
    /// from the real-time thread.
    pub fn new(
        profile: AudioProfile,
        position: PositionWriter,
        waveform: WaveformWriter,
        takes: TakeWriter,
    ) -> Self {
        let block = profile.block_size as usize;
        assert!(block > 0, "an engine renders nothing block by block");
        let mut gain = Gain::unity();
        gain.prepare(profile.sample_rate);
        let mut output = Gain::unity();
        output.prepare(profile.sample_rate);

        Self {
            buffer: LoopBuffer::for_profile(profile),
            position,
            waveform,
            takes,
            gained: vec![0.0; block].into_boxed_slice(),
            transport: Transport::default(),
            playhead: 0,
            layer_open: true,
            gain,
            output,
            bars: None,
        }
    }

    fn move_to(&mut self, transport: Transport) {
        let was_writing = self.writing_the_loop();
        if transport.plays_loop() && !self.transport.plays_loop() {
            self.playhead = 0;
        }
        if transport == Transport::Overdubbing && self.transport != Transport::Overdubbing {
            self.layer_open = self.buffer.depth() < LoopBuffer::LAYERS;
            if self.layer_open {
                self.buffer.overdub(self.playhead);
            }
        }

        self.transport = transport;
        if self.writing_the_loop() != was_writing {
            self.hand_over_the_take();
        }
    }

    const fn writing_the_loop(&self) -> bool {
        self.transport.captures_input() && self.layer_open
    }

    fn hand_over_the_take(&mut self) {
        if self.writing_the_loop() {
            self.takes.abandon();
        } else {
            self.takes.begin(&self.buffer, self.bars);
        }
    }

    fn mix_block(&mut self, captured: &[f32], playing: &mut [f32]) {
        let frames = captured.len();
        self.gained[..frames].copy_from_slice(captured);
        self.gain.apply(&mut self.gained[..frames]);
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
        self.buffer.resummarise(frames);
        self.takes.advance(&self.buffer, frames);

        for (played, level) in playing.iter_mut().zip(gained) {
            *played = held(*played + level);
        }
        self.output.apply(playing);
    }

    fn publish(&mut self) {
        self.position.publish(LoopPosition::new(
            frame_count(self.playhead),
            frame_count(self.buffer.len()),
            self.buffer.depth(),
        ));
        self.waveform.publish(self.buffer.waveform());
    }
}

fn frame_count(frames: usize) -> u32 {
    u32::try_from(frames).unwrap_or(u32::MAX)
}

impl AudioPath for LoopEngine {
    /// Spreads both gains' ramps over the rate the device granted, which is all
    /// there is to prepare: the loop and the scratch are sized from the profile
    /// the engine was built with, and what the device granted reaches the block
    /// as the frames it was handed rather than as a number stated up front.
    ///
    /// That is the honest reading of the two, since the block a
    /// [`StreamConfig`] states is not a bound on the block a callback gets. A
    /// longer one is worked in chunks, and a shorter one takes a smaller share
    /// of the take crossing.
    fn prepare(&mut self, config: StreamConfig) {
        self.gain.prepare(config.sample_rate);
        self.output.prepare(config.sample_rate);
    }

    /// Plays the loop under the player's input, holds the sum inside full
    /// scale, and publishes the playhead the block ended on.
    ///
    /// The ceiling is on the whole block, not the loop alone, so a gained
    /// input over [`HELD_ABOVE`](crate::audio::HELD_ABOVE) is curved too.
    ///
    /// Two lengths become the shorter of them, and a block longer than the
    /// scratch is worked in chunks: neither is a panic on the audio thread. A
    /// block also carries the loop's summary forward where an undo left it
    /// behind, so the shape heals within a lap.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
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

    /// Answers every command there is: the loop is what the transport, undo and
    /// clear are for, and the input reaches the loop through the gain and the
    /// mute, so a composition offering it a command first leaves nothing for
    /// anything behind it.
    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::SetTransport(transport) => self.move_to(transport),
            Command::SetMuted(muted) => self.output.set_muted(muted),
            Command::SetGain(gain) => self.gain.set_target(gain),
            Command::SetBars(bars) => self.bars = bars,
            Command::Undo => {
                if self.buffer.undo() {
                    self.layer_open = false;
                    self.hand_over_the_take();
                }
            }
            Command::Clear => {
                self.buffer.clear();
                self.layer_open = true;
                self.playhead = 0;
                self.hand_over_the_take();
            }
        }

        true
    }
}
