//! The click a player plays against, placed on the beat rather than on the
//! block.
//!
//! An onset lands on a sample. Quantising a click to the block it arrived in
//! puts it a few milliseconds either side of the beat and leaves it wandering
//! against the audio it is there to line up with, which is the one thing a
//! metronome may not do — so a beat is turned into an offset into the block and
//! the click is summed in from there.
//!
//! Nothing is synthesised per block (invariant 2). The envelope is rendered
//! where the trait allows a path to allocate and summed in where it does not,
//! and a click too long for one block carries on into the next from where it
//! left off.

use std::f32::consts::TAU;

use crate::seq::ScheduleReader;

use super::{AudioPath, Command, SampleClockReader, StreamConfig, held};

const CLICK_HERTZ: f32 = 1_000.0;
const CLICK_MILLISECONDS: u32 = 8;
const MILLISECONDS_IN_A_SECOND: u32 = 1_000;
const CLICK_LEVEL: f32 = 0.5;
const DECAY_PER_SECOND: f32 = 400.0;

/// A path with a click on the beat over it.
///
/// It plays what the path under it plays and sums a short decaying tone in at
/// every beat the block covers, which is also what keeps the click outside the
/// loop's own mute: a reference is still a reference over a muted take.
///
/// Silent until a tempo exists — the beats arrive on a
/// [`ScheduleReader`](crate::seq::ScheduleReader) that holds none until the
/// player has stated one.
///
/// ```
/// use motif::audio::{AudioPath, Metronome, Passthrough, StreamConfig, sample_clock};
/// use motif::seq::{BeatGrid, beat_schedule};
///
/// let (_frames, elapsed) = sample_clock(48_000);
/// let (mut beats, schedule) = beat_schedule();
/// let mut metronome = Metronome::over(schedule, elapsed, Passthrough::new());
/// metronome.prepare(StreamConfig {
///     sample_rate: 48_000,
///     block_size: 8,
///     input_channels: 1,
///     output_channels: 1,
/// });
///
/// let mut grid = BeatGrid::new(48_000);
/// for beat in [2, 24_002] {
///     assert!(grid.push(beat));
/// }
/// beats.follow(&grid, 1);
///
/// let mut playing = [0.0; 8];
/// metronome.render(&[0.0; 8], &mut playing);
///
/// assert_eq!(playing[1], 0.0);
/// assert_ne!(playing[2], 0.0);
/// ```
pub struct Metronome<P> {
    schedule: ScheduleReader,
    elapsed: SampleClockReader,
    click: Vec<f32>,
    struck: usize,
    path: P,
}

impl<P: AudioPath> Metronome<P> {
    /// `path`, clicking on the beats `schedule` holds, timed by `elapsed`.
    ///
    /// `elapsed` is the clock a beat was timestamped against, which is what
    /// makes a beat and a block comparable at all. It has to be the count of
    /// frames played *before* this block, so a metronome belongs inside the
    /// [`Counting`](super::Counting) that keeps the clock rather than around
    /// it.
    ///
    /// The click itself is rendered by [`prepare`](AudioPath::prepare), so one
    /// built and never prepared plays only what is under it.
    pub const fn over(schedule: ScheduleReader, elapsed: SampleClockReader, path: P) -> Self {
        Self {
            schedule,
            elapsed,
            click: Vec::new(),
            struck: 0,
            path,
        }
    }

    fn sound(&mut self, playing: &mut [f32]) {
        let sounding = self.click.get(self.struck..).unwrap_or_default();
        for (played, &click) in playing.iter_mut().zip(sounding) {
            *played = held(*played + click);
        }

        self.struck += playing.len().min(sounding.len());
    }
}

fn click_of(sample_rate: u32) -> Vec<f32> {
    let frames = sample_rate * CLICK_MILLISECONDS / MILLISECONDS_IN_A_SECOND;
    (0..frames)
        .map(|frame| {
            let seconds = frame as f32 / sample_rate as f32;
            CLICK_LEVEL * (-DECAY_PER_SECOND * seconds).exp() * (TAU * CLICK_HERTZ * seconds).cos()
        })
        .collect()
}

impl<P: AudioPath> AudioPath for Metronome<P> {
    /// Renders the click at the rate the device granted, which is the one place
    /// a path may allocate, and leaves no click part-played across a stream
    /// that was reopened under it.
    fn prepare(&mut self, config: StreamConfig) {
        self.path.prepare(config);
        self.click = click_of(config.sample_rate);
        self.struck = self.click.len();
    }

    /// Plays the path under it, then sums the click in at every beat this block
    /// covers and carries on one the block before it started.
    ///
    /// Blocks are laid end to end by the clock they are timed against, so a
    /// beat falls in exactly one of them and needs no record of what has been
    /// struck already. The schedule is read once and is a fixed number of
    /// beats, so the work is the same on every block.
    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.path.render(captured, playing);

        let frames = captured.len().min(playing.len());
        let started = self.elapsed.read();
        self.sound(&mut playing[..frames]);

        for beat in self.schedule.read().beats() {
            let Some(offset) = beat.checked_sub(started) else {
                continue;
            };
            let Ok(offset) = usize::try_from(offset) else {
                continue;
            };
            if offset < frames {
                self.struck = 0;
                self.sound(&mut playing[offset..frames]);
            }
        }
    }

    /// Answers whatever the path it holds answers, having nothing of its own to
    /// take: what a metronome clicks on is a grid the player states, and it
    /// reaches the callback on a schedule rather than as a command.
    fn apply(&mut self, command: Command) -> bool {
        self.path.apply(command)
    }
}
