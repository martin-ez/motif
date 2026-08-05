//! The synthetic fixture set: short renders whose ground truth is known by
//! construction rather than tapped by a human.
//!
//! The standard annotated corpora carry licence terms that rule out vendoring
//! their audio into a public repository. Synthesis sidesteps that, and yields
//! exact beat positions as a by-product of placing the sounds.
//!
//! Rendering is deterministic: the same call produces the same samples, so the
//! files checked in beside this module can be checked against it.

use crate::fixtures::Beat;
use std::time::Duration;

/// The sample rate every synthetic fixture is rendered at.
///
/// 8 kHz, the rate Ellis's dynamic-programming beat tracker resamples to before
/// computing its onset envelope: percussive attacks survive it intact, and it
/// is what keeps the committed set inside the size the epic commits to.
pub const SAMPLE_RATE: u32 = 8_000;

/// Which of the two synthesised sounds an onset plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// A low decaying tone under a short transient, marking the start of a bar.
    Accent,
    /// A short noise burst, marking anything else.
    Tick,
}

/// A sound in a fixture: when it happens, and what it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Onset {
    /// Where it falls, measured from the start of the audio.
    pub at: Duration,
    /// The sound it plays.
    pub voice: Voice,
}

/// One synthetic fixture: the beat grid it was built on, the onsets placed
/// against that grid, and the audio they render to.
///
/// The beats are the ground truth. They are not always where the sounds are —
/// a syncopated fixture places most of its onsets between them, which is the
/// case that separates a beat tracker from an onset detector.
///
/// ```
/// use motif::fixtures::synth;
///
/// let fixture = &synth::set()[0];
/// let annotation: motif::fixtures::Annotation = fixture.annotation_text().parse()?;
///
/// assert_eq!(annotation.beats(), fixture.beats());
/// # Ok::<(), motif::fixtures::AnnotationError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    name: &'static str,
    description: &'static str,
    beats: Vec<Beat>,
    onsets: Vec<Onset>,
    samples: Vec<i16>,
}

impl Fixture {
    /// What the fixture is called, and what its two files are named after.
    pub fn name(&self) -> &str {
        self.name
    }

    /// What the fixture is for, in a phrase.
    pub fn description(&self) -> &str {
        self.description
    }

    /// The ground truth: every beat, in order, with the downbeats identified.
    pub fn beats(&self) -> &[Beat] {
        &self.beats
    }

    /// Every sound the audio contains, in the order they play.
    pub fn onsets(&self) -> &[Onset] {
        &self.onsets
    }

    /// The rendered audio, one mono sample per frame at [`SAMPLE_RATE`].
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    /// The audio as a canonical 16-bit mono PCM WAV file.
    pub fn wav_bytes(&self) -> Vec<u8> {
        let data = self.samples.len() * size_of::<i16>();
        let mut wav = Vec::with_capacity(44 + data);

        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + data) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data as u32).to_le_bytes());
        for sample in &self.samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }

        wav
    }

    /// The ground truth in the format [`Annotation`](crate::fixtures::Annotation)
    /// reads, ready to be written beside the audio.
    ///
    /// Timestamps carry six decimals, which is exact for a position on the
    /// sample grid at [`SAMPLE_RATE`], so the file names the same instant the
    /// audio does.
    pub fn annotation_text(&self) -> String {
        let mut text = format!("# {}: {}\n", self.name, self.description);
        text.push_str("# rendered by `cargo run --example generate-fixtures`\n");
        for beat in &self.beats {
            let kind = if beat.is_downbeat { "downbeat" } else { "beat" };
            text.push_str(&format!("{:.6} {kind}\n", beat.at.as_secs_f64()));
        }

        text
    }
}

/// Every synthetic fixture, in the order they are written.
///
/// Each one breaks a different assumption: the steady pairs are the baseline,
/// the waltz denies that a bar is four beats, the ramp and rubato passage deny
/// that a tempo is a number, the syncopated case that a sound implies a beat.
///
/// The rubato passage pulls 130 ms against its pulse — more than the +/-70 ms
/// scoring window — so a steady-tempo tracker is measurably wrong, not lucky.
///
/// At 16 KB of PCM per second, the 512 KiB the set is held under is about half
/// a minute of audio, so rendering rejects a fixture over ten seconds long.
pub fn set() -> Vec<Fixture> {
    vec![
        rendered(
            "steady-90-4-4",
            "4/4 at 90 BPM",
            steady(90.0, 4, 2),
            on_every_beat,
        ),
        rendered(
            "steady-120-4-4",
            "4/4 at 120 BPM",
            steady(120.0, 4, 2),
            on_every_beat,
        ),
        rendered(
            "steady-150-4-4",
            "4/4 at 150 BPM",
            steady(150.0, 4, 2),
            on_every_beat,
        ),
        rendered(
            "waltz-150-3-4",
            "3/4 at 150 BPM, so a bar is not four beats",
            steady(150.0, 3, 4),
            on_every_beat,
        ),
        rendered(
            "ramp-100-140-4-4",
            "4/4 accelerating from 100 to 140 BPM",
            ramp(100.0, 140.0, 4, 2),
            on_every_beat,
        ),
        rendered(
            "rubato-110-4-4",
            "4/4 around 110 BPM, pushing and pulling against the pulse",
            rubato(110.0, 4, 2),
            on_every_beat,
        ),
        rendered(
            "syncopated-120-4-4",
            "4/4 at 120 BPM with the sounds mostly between the beats",
            steady(120.0, 4, 2),
            off_the_beat,
        ),
    ]
}

fn rendered(
    name: &'static str,
    description: &'static str,
    beats: Vec<Beat>,
    place: fn(&[Beat]) -> Vec<Onset>,
) -> Fixture {
    let onsets = place(&beats);
    let samples = render(&onsets, seed_of(name));

    Fixture {
        name,
        description,
        beats,
        onsets,
        samples,
    }
}

fn steady(tempo: f64, beats_per_bar: usize, bars: usize) -> Vec<Beat> {
    let period = 60.0 / tempo;
    let times = (0..beats_per_bar * bars).map(|index| index as f64 * period);

    grid(times, beats_per_bar)
}

fn ramp(from: f64, to: f64, beats_per_bar: usize, bars: usize) -> Vec<Beat> {
    let count = beats_per_bar * bars;
    let intervals = count - 1;
    let mut at = 0.0;
    let mut times = vec![at];
    for interval in 0..intervals {
        let tempo = from + (to - from) * interval as f64 / (intervals - 1) as f64;
        at += 60.0 / tempo;
        times.push(at);
    }

    grid(times.into_iter(), beats_per_bar)
}

const RUBATO_PULL: f64 = 0.13;

fn rubato(tempo: f64, beats_per_bar: usize, bars: usize) -> Vec<Beat> {
    let period = 60.0 / tempo;
    let count = beats_per_bar * bars;
    let times = (0..count).map(|index| {
        let phase = std::f64::consts::TAU * index as f64 / count as f64;
        index as f64 * period + RUBATO_PULL * phase.sin()
    });

    grid(times, beats_per_bar)
}

fn grid(times: impl Iterator<Item = f64>, beats_per_bar: usize) -> Vec<Beat> {
    times
        .enumerate()
        .map(|(index, seconds)| Beat {
            at: on_the_sample_grid(seconds),
            is_downbeat: index % beats_per_bar == 0,
        })
        .collect()
}

fn on_the_sample_grid(seconds: f64) -> Duration {
    let frame = (seconds * f64::from(SAMPLE_RATE)).round() as u64;

    Duration::from_nanos(frame * 1_000_000_000 / u64::from(SAMPLE_RATE))
}

fn on_every_beat(beats: &[Beat]) -> Vec<Onset> {
    beats
        .iter()
        .map(|beat| Onset {
            at: beat.at,
            voice: if beat.is_downbeat {
                Voice::Accent
            } else {
                Voice::Tick
            },
        })
        .collect()
}

fn off_the_beat(beats: &[Beat]) -> Vec<Onset> {
    let intervals: Vec<Duration> = beats
        .windows(2)
        .map(|pair| pair[1].at - pair[0].at)
        .collect();
    let mut onsets: Vec<Onset> = beats
        .iter()
        .filter(|beat| beat.is_downbeat)
        .map(|beat| Onset {
            at: beat.at,
            voice: Voice::Accent,
        })
        .collect();

    for (index, beat) in beats.iter().enumerate() {
        if let Some(interval) = intervals.get(index).or_else(|| intervals.last()) {
            onsets.push(Onset {
                at: halfway_past(beat.at, *interval),
                voice: Voice::Tick,
            });
        }
    }
    onsets.sort_by_key(|onset| onset.at);

    onsets
}

fn halfway_past(beat: Duration, interval: Duration) -> Duration {
    on_the_sample_grid(beat.as_secs_f64() + interval.as_secs_f64() / 2.0)
}

const TAIL: Duration = Duration::from_millis(300);
const LONGEST: Duration = Duration::from_secs(10);
const ACCENT_FREQUENCY: f64 = 60.0;
const ACCENT_DECAY: f64 = 0.10;
const ACCENT_LEVEL: f64 = 0.85;
const TRANSIENT_DECAY: f64 = 0.006;
const TRANSIENT_LEVEL: f64 = 0.30;
const TICK_DECAY: f64 = 0.030;
const TICK_LEVEL: f64 = 0.55;

fn render(onsets: &[Onset], seed: u32) -> Vec<i16> {
    let last = onsets
        .iter()
        .map(|onset| onset.at)
        .max()
        .unwrap_or_default();
    let length = last + TAIL;
    assert!(
        length <= LONGEST,
        "a fixture running {length:?} cannot belong to a set held under its size ceiling"
    );
    let mut signal = vec![0.0; frames(length)];
    let mut noise = Noise::from(seed);

    for onset in onsets {
        let start = frames(onset.at);
        for (offset, frame) in signal[start..].iter_mut().take(frames(TAIL)).enumerate() {
            let elapsed = offset as f64 / f64::from(SAMPLE_RATE);
            *frame += match onset.voice {
                Voice::Accent => {
                    ACCENT_LEVEL
                        * (-elapsed / ACCENT_DECAY).exp()
                        * (std::f64::consts::TAU * ACCENT_FREQUENCY * elapsed).sin()
                        + TRANSIENT_LEVEL * (-elapsed / TRANSIENT_DECAY).exp() * noise.next()
                }
                Voice::Tick => TICK_LEVEL * (-elapsed / TICK_DECAY).exp() * noise.next(),
            };
        }
    }

    signal
        .into_iter()
        .map(|frame| (frame.clamp(-1.0, 1.0) * f64::from(i16::MAX)) as i16)
        .collect()
}

fn frames(at: Duration) -> usize {
    (at.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as usize
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;

        f64::from(self.0) / f64::from(u32::MAX) * 2.0 - 1.0
    }
}

impl From<u32> for Noise {
    fn from(seed: u32) -> Self {
        Self(seed | 1)
    }
}

fn seed_of(name: &str) -> u32 {
    name.bytes().fold(2_166_136_261, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}
