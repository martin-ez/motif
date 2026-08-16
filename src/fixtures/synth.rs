//! The synthetic fixture set: short renders whose ground truth is known by
//! construction rather than tapped by a human.
//!
//! The standard annotated corpora carry licence terms that rule out vendoring
//! their audio into a public repository. Synthesis sidesteps that, and yields
//! exact beat positions as a by-product of placing the sounds.
//!
//! Rendering is deterministic: the same call produces the same samples, so the
//! files checked in beside this module can be checked against it.
//!
//! [`set`] is those committed files, which the repository's size bounds. [`drawn`]
//! is a set rendered from a seed and never written down, which only patience
//! bounds, and which carries the [`Recipe`] a report bands its aggregate by.

use crate::fixtures::{
    Axis, Beat, Chord, ChordLabel, Drift, Note, PitchClass, Quality, Recipe, Texture,
};
use std::time::Duration;

/// The sample rate every synthetic fixture is rendered at.
///
/// 8 kHz, the rate Ellis's dynamic-programming beat tracker resamples to before
/// computing its onset envelope: percussive attacks survive it intact, and it
/// is what keeps the committed set inside the size the epic commits to.
pub const SAMPLE_RATE: u32 = 8_000;

/// Which of the synthesised sounds an onset plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// A low decaying tone under a short transient, marking the start of a bar.
    Accent,
    /// A short noise burst, marking anything else.
    Tick,
    /// A pitched tone, held until it is released.
    Tone {
        /// Which note it plays, as a MIDI note number.
        pitch: u8,
        /// Where it stops, measured from the start of the audio.
        until: Duration,
    },
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
#[derive(Debug, Clone, PartialEq)]
pub struct Fixture {
    name: String,
    description: String,
    recipe: Recipe,
    beats: Vec<Beat>,
    chords: Vec<Chord>,
    notes: Vec<Note>,
    onsets: Vec<Onset>,
    samples: Vec<i8>,
}

impl Fixture {
    /// What the fixture is called, and what its two files are named after.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the fixture is for, in a phrase.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The parameters it was rendered from.
    ///
    /// This is what makes a report diagnosable past its aggregate: a candidate
    /// that scored badly scored badly somewhere, and a recipe is what names
    /// where.
    pub fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    /// The ground truth: every beat, in order, with the downbeats identified.
    pub fn beats(&self) -> &[Beat] {
        &self.beats
    }

    /// The harmony sounding over the beats, one span per bar where there is any.
    pub fn chords(&self) -> &[Chord] {
        &self.chords
    }

    /// The notes of the monophonic line, where the fixture plays one.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Every sound the audio contains, in the order they play.
    pub fn onsets(&self) -> &[Onset] {
        &self.onsets
    }

    /// The rendered audio, one mono sample per frame at [`SAMPLE_RATE`].
    ///
    /// Eight bits: quantisation noise lands around -42 dBFS against these
    /// near-full-scale clicks, far under what an onset envelope resolves. At
    /// the 8 KB per second that leaves, the 576 KiB [`set`] is held under is
    /// about seventy seconds of audio across the whole of it — a ceiling a
    /// drawn fixture does not pay, since it is never written down.
    pub fn samples(&self) -> &[i8] {
        &self.samples
    }

    /// The audio as a canonical 8-bit mono PCM WAV file.
    ///
    /// Eight-bit RIFF samples are unsigned, so silence is stored at 128 rather
    /// than at zero.
    pub fn wav_bytes(&self) -> Vec<u8> {
        let data = self.samples.len();
        let mut wav = Vec::with_capacity(44 + data);

        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + data) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data as u32).to_le_bytes());
        for sample in &self.samples {
            wav.push(unsigned(*sample));
        }

        wav
    }

    /// The ground truth in the format [`Annotation`](crate::fixtures::Annotation)
    /// reads, ready to be written beside the audio.
    ///
    /// Timestamps carry six decimals, which is exact for a position on the
    /// sample grid at [`SAMPLE_RATE`], so the file names the same instant the
    /// audio does. The kinds are written in blocks, and the chords end with the
    /// `N` that says where the harmony stops.
    pub fn annotation_text(&self) -> String {
        let mut text = format!("# {}: {}\n", self.name, self.description);
        text.push_str("# rendered by `cargo run --example generate-fixtures`\n");
        for beat in &self.beats {
            let kind = if beat.is_downbeat { "downbeat" } else { "beat" };
            text.push_str(&entry(beat.at, kind));
        }
        for chord in &self.chords {
            text.push_str(&entry(chord.from, &format!("chord {}", chord.label)));
        }
        if let Some(last) = self.chords.last() {
            text.push_str(&entry(last.to, "chord N"));
        }
        for note in &self.notes {
            let played = format!("note {} {:.6}", note.pitch, note.offset.as_secs_f64());
            text.push_str(&entry(note.onset, &played));
        }

        text
    }
}

/// Every synthetic fixture, in the order they are written.
///
/// The steady pair is the baseline, and each of the rest denies something: that
/// a bar is four beats, that a tempo is a number, that a sound implies a beat,
/// or that what an analyser is handed is percussion.
///
/// The rubato passage pulls 130 ms against its pulse — past the +/-70 ms
/// scoring window — so a steady-tempo tracker is measurably wrong, not lucky.
///
/// Four bars each is what the harness's resolution rests on: one misread bar
/// moves its aggregate by a thirty-sixth rather than a fourteenth.
pub fn set() -> Vec<Fixture> {
    vec![
        built(
            "steady-90-4-4",
            "4/4 at 90 BPM",
            clicks(90.0, 4, ON_THE_BEAT),
        ),
        built(
            "steady-120-4-4",
            "4/4 at 120 BPM",
            clicks(120.0, 4, ON_THE_BEAT),
        ),
        built(
            "steady-150-4-4",
            "4/4 at 150 BPM",
            clicks(150.0, 4, ON_THE_BEAT),
        ),
        built(
            "waltz-150-3-4",
            "3/4 at 150 BPM, so a bar is not four beats",
            clicks(150.0, 3, ON_THE_BEAT),
        ),
        built(
            "ramp-100-140-4-4",
            "4/4 accelerating from 100 to 140 BPM",
            Recipe {
                drift: Drift::Ramp { to: 140.0 },
                ..clicks(100.0, 4, ON_THE_BEAT)
            },
        ),
        built(
            "rubato-110-4-4",
            "4/4 around 110 BPM, pushing and pulling against the pulse",
            Recipe {
                drift: Drift::Rubato { pull: RUBATO_PULL },
                ..clicks(110.0, 4, ON_THE_BEAT)
            },
        ),
        built(
            "syncopated-120-4-4",
            "4/4 at 120 BPM with the sounds mostly between the beats",
            clicks(120.0, 4, BETWEEN_THE_BEATS),
        ),
        built(
            "chords-150-4-4",
            "4/4 at 150 BPM voicing a chord to the bar",
            committed(150.0, 4, Texture::Chords),
        ),
        built(
            "line-150-4-4",
            "4/4 at 150 BPM playing a monophonic line",
            committed(150.0, 4, Texture::Line),
        ),
    ]
}

const COMMITTED_BARS: usize = 4;
const SHARP: f64 = 1.0;
const ON_THE_BEAT: f64 = 0.0;
const BETWEEN_THE_BEATS: f64 = 1.0;
const ONE_TO_THE_BEAT: usize = 1;
const NONE_UNSOUNDED: f64 = 0.0;

fn clicks(tempo: f64, meter: usize, syncopation: f64) -> Recipe {
    committed(
        tempo,
        meter,
        Texture::Percussion {
            sharpness: SHARP,
            density: ONE_TO_THE_BEAT,
            dropout: NONE_UNSOUNDED,
            syncopation,
        },
    )
}

fn committed(tempo: f64, meter: usize, texture: Texture) -> Recipe {
    Recipe {
        tempo,
        meter,
        bars: COMMITTED_BARS,
        drift: Drift::Steady,
        texture,
    }
}

/// Render one fixture from `recipe`, called `name`.
///
/// Deterministic in both: the same pair yields the same beats, the same onsets
/// and the same samples. That is what lets a set be drawn on demand rather than
/// committed, since a candidate scored against it today is scored against the
/// same audio tomorrow.
pub fn rendered(name: &str, recipe: Recipe) -> Fixture {
    built(name, &described(&recipe), recipe)
}

struct Content {
    chords: Vec<Chord>,
    notes: Vec<Note>,
    onsets: Vec<Onset>,
}

fn built(name: &str, description: &str, recipe: Recipe) -> Fixture {
    let beats = grid_of(&recipe);
    let seed = seed_of(name);
    let Content {
        chords,
        notes,
        onsets,
    } = sounded(&recipe, &beats);
    let samples = render(&onsets, seed, sharpness_of(&recipe.texture));

    Fixture {
        name: name.to_owned(),
        description: description.to_owned(),
        recipe,
        beats,
        chords,
        notes,
        onsets,
        samples,
    }
}

fn described(recipe: &Recipe) -> String {
    Axis::ALL
        .iter()
        .filter_map(|axis| {
            axis.level(recipe)
                .map(|level| format!("{} {}", axis.named(), level.trim()))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn grid_of(recipe: &Recipe) -> Vec<Beat> {
    match recipe.drift {
        Drift::Steady => steady(recipe.tempo, recipe.meter, recipe.bars),
        Drift::Ramp { to } => ramp(recipe.tempo, to, recipe.meter, recipe.bars),
        Drift::Rubato { pull } => rubato(recipe.tempo, pull, recipe.meter, recipe.bars),
    }
}

fn sounded(recipe: &Recipe, beats: &[Beat]) -> Content {
    match recipe.texture {
        Texture::Percussion {
            density,
            dropout,
            syncopation,
            ..
        } => percussive(struck_over(beats, density, dropout, syncopation)),
        Texture::Chords => one_chord_per_bar(beats),
        Texture::Line => a_monophonic_line(beats),
    }
}

fn sharpness_of(texture: &Texture) -> f64 {
    match texture {
        Texture::Percussion { sharpness, .. } => *sharpness,
        Texture::Chords | Texture::Line => SHARP,
    }
}

/// The seeds a set is drawn from while an approach is being developed.
pub const DEVELOPMENT: [u32; 3] = [1, 2, 3];

/// The seed held back from those, for the figure that ranks two approaches.
///
/// A candidate tuned against the development seeds has already seen what they
/// draw, and a figure quoted from them is the number it was tuned to. Drawing
/// from this one before there is a result to report is what makes the
/// comparison stop meaning anything.
pub const EVALUATION: u32 = 4;

/// Draw `count` fixtures from `seed` and render them.
///
/// Every fixture is percussive and runs the same number of bars, so a row that
/// scored badly did so on its parameters rather than on its length. What varies
/// is what [`Axis`] names, which is what a report can then band by.
///
/// ```
/// use motif::fixtures::synth;
///
/// let set = synth::drawn(synth::DEVELOPMENT[0], 2);
///
/// assert_eq!(set.len(), 2);
/// assert_eq!(set, synth::drawn(synth::DEVELOPMENT[0], 2));
/// ```
pub fn drawn(seed: u32, count: usize) -> Vec<Fixture> {
    let set = format!("drawn-{seed:08x}");
    let mut draw = Noise::from(seed_of(&set));

    (0..count)
        .map(|index| rendered(&format!("{set}-{index:03}"), drawn_from(&mut draw)))
        .collect()
}

const DRAWN_BARS: usize = 8;
const TEMPI: [f64; 5] = [80.0, 100.0, 120.0, 140.0, 160.0];
const METERS: [usize; 3] = [3, 4, 5];
const DRIFTS: [Kind; 3] = [Kind::Steady, Kind::Ramp, Kind::Rubato];
const SHARPNESSES: [f64; 3] = [1.0, 0.7, 0.4];
const DENSITIES: [usize; 2] = [1, 2];
const DROPOUTS: [f64; 3] = [0.0, 0.15, 0.3];
const SYNCOPATIONS: [f64; 3] = [0.0, 0.25, 0.5];
const RAMP_REACH: f64 = 1.4;

enum Kind {
    Steady,
    Ramp,
    Rubato,
}

fn drawn_from(draw: &mut Noise) -> Recipe {
    let tempo = *pick(&TEMPI, draw);

    Recipe {
        tempo,
        meter: *pick(&METERS, draw),
        bars: DRAWN_BARS,
        drift: match pick(&DRIFTS, draw) {
            Kind::Steady => Drift::Steady,
            Kind::Ramp => Drift::Ramp {
                to: tempo * RAMP_REACH,
            },
            Kind::Rubato => Drift::Rubato { pull: RUBATO_PULL },
        },
        texture: Texture::Percussion {
            sharpness: *pick(&SHARPNESSES, draw),
            density: *pick(&DENSITIES, draw),
            dropout: *pick(&DROPOUTS, draw),
            syncopation: *pick(&SYNCOPATIONS, draw),
        },
    }
}

fn pick<'a, T>(levels: &'a [T], draw: &mut Noise) -> &'a T {
    &levels[draw.below(levels.len())]
}

fn percussive(onsets: Vec<Onset>) -> Content {
    Content {
        chords: Vec::new(),
        notes: Vec::new(),
        onsets,
    }
}

const PROGRESSION: [(u8, Quality); 4] = [
    (0, Quality::Maj),
    (9, Quality::Min),
    (5, Quality::Maj),
    (7, Quality::Dom7),
];

const LOWEST_ROOT: u8 = 60;

fn one_chord_per_bar(beats: &[Beat]) -> Content {
    let chords: Vec<Chord> = bars(beats)
        .into_iter()
        .zip(PROGRESSION)
        .map(|((from, to), (semitone, quality))| Chord {
            label: ChordLabel::Sounding(PitchClass::from_semitone(semitone), quality),
            from,
            to,
        })
        .collect();
    let lasting = lasting(beats);
    let mut onsets = Vec::new();

    for chord in &chords {
        let voicing = voicing(chord);
        for (index, beat) in beats.iter().enumerate() {
            if !struck_under(chord, beat) {
                continue;
            }
            for pitch in &voicing {
                onsets.push(Onset {
                    at: beat.at,
                    voice: Voice::Tone {
                        pitch: *pitch,
                        until: detached(beat.at, lasting[index]),
                    },
                });
            }
        }
    }

    Content {
        chords,
        notes: Vec::new(),
        onsets,
    }
}

fn struck_under(chord: &Chord, beat: &Beat) -> bool {
    chord.from <= beat.at && beat.at < chord.to
}

fn lasting(beats: &[Beat]) -> Vec<Duration> {
    let mut spans: Vec<Duration> = beats
        .windows(2)
        .map(|pair| pair[1].at - pair[0].at)
        .collect();
    if let Some(last) = spans.last().copied() {
        spans.push(last);
    }

    spans
}

fn detached(from: Duration, span: Duration) -> Duration {
    from + span - span / DETACHED
}

fn bars(beats: &[Beat]) -> Vec<(Duration, Duration)> {
    let starts: Vec<Duration> = beats
        .iter()
        .filter(|beat| beat.is_downbeat)
        .map(|beat| beat.at)
        .collect();
    let ends = starts.iter().skip(1).copied().chain(past_the_end(beats));

    starts.iter().copied().zip(ends).collect()
}

fn past_the_end(beats: &[Beat]) -> Option<Duration> {
    let last = beats.last()?;

    Some(last.at + *lasting(beats).last()?)
}

fn voicing(chord: &Chord) -> Vec<u8> {
    let ChordLabel::Sounding(root, quality) = chord.label else {
        return Vec::new();
    };
    let intervals: &[u8] = match quality {
        Quality::Maj => &[0, 4, 7],
        Quality::Min => &[0, 3, 7],
        Quality::Dim => &[0, 3, 6],
        Quality::Aug => &[0, 4, 8],
        Quality::Maj7 => &[0, 4, 7, 11],
        Quality::Min7 => &[0, 3, 7, 10],
        Quality::Dom7 => &[0, 4, 7, 10],
    };

    intervals
        .iter()
        .map(|interval| LOWEST_ROOT + root.semitone() + interval)
        .collect()
}

const LINE: [(usize, usize, u8); 12] = [
    (0, 1, 60),
    (1, 1, 62),
    (2, 2, 64),
    (4, 1, 65),
    (5, 1, 64),
    (6, 2, 62),
    (8, 1, 67),
    (9, 1, 65),
    (10, 2, 64),
    (12, 1, 62),
    (13, 1, 60),
    (14, 2, 55),
];

const DETACHED: u32 = 10;

fn a_monophonic_line(beats: &[Beat]) -> Content {
    let lasting = lasting(beats);
    let notes: Vec<Note> = LINE
        .into_iter()
        .take_while(|(index, length, _)| index + length <= lasting.len())
        .map(|(index, length, pitch)| Note {
            pitch,
            onset: beats[index].at,
            offset: detached(beats[index].at, lasting[index..index + length].iter().sum()),
        })
        .collect();
    let onsets = notes
        .iter()
        .map(|note| Onset {
            at: note.onset,
            voice: Voice::Tone {
                pitch: note.pitch,
                until: note.offset,
            },
        })
        .collect();

    Content {
        chords: Vec::new(),
        notes,
        onsets,
    }
}

fn steady(tempo: f64, beats_per_bar: usize, bars: usize) -> Vec<Beat> {
    let period = 60.0 / tempo;
    let times = (0..beats_per_bar * bars).map(|index| index as f64 * period);

    grid(times, beats_per_bar)
}

fn ramp(from: f64, to: f64, beats_per_bar: usize, bars: usize) -> Vec<Beat> {
    let count = beats_per_bar * bars;
    let intervals = count.saturating_sub(1);
    let spread = intervals.saturating_sub(1).max(1);
    let spans = (0..intervals).map(|interval| {
        let tempo = from + (to - from) * interval as f64 / spread as f64;
        60.0 / tempo
    });
    let times = std::iter::once(0.0)
        .chain(spans.scan(0.0, |at, span| {
            *at += span;
            Some(*at)
        }))
        .take(count);

    grid(times, beats_per_bar)
}

const RUBATO_PULL: f64 = 0.13;

fn rubato(tempo: f64, pull: f64, beats_per_bar: usize, bars: usize) -> Vec<Beat> {
    let period = 60.0 / tempo;
    let count = beats_per_bar * bars;
    let times = (0..count).map(|index| {
        let phase = std::f64::consts::TAU * index as f64 / count as f64;
        index as f64 * period + pull * phase.sin()
    });

    grid(times, beats_per_bar)
}

fn grid(times: impl Iterator<Item = f64>, beats_per_bar: usize) -> Vec<Beat> {
    let times: Vec<f64> = times.collect();
    if !advances(&times) {
        return Vec::new();
    }

    times
        .into_iter()
        .enumerate()
        .map(|(index, seconds)| Beat {
            at: on_the_sample_grid(seconds),
            is_downbeat: index % beats_per_bar == 0,
        })
        .collect()
}

fn advances(times: &[f64]) -> bool {
    times.iter().all(|at| at.is_finite() && *at >= 0.0)
        && times.windows(2).all(|pair| pair[1] > pair[0])
}

const NANOS_PER_SECOND: u64 = 1_000_000_000;

fn on_the_sample_grid(seconds: f64) -> Duration {
    let rate = u64::from(SAMPLE_RATE);
    let frame = (seconds * f64::from(SAMPLE_RATE)).round() as u64;
    let over = ((frame % rate) * NANOS_PER_SECOND / rate) as u32;

    Duration::new(frame / rate, over)
}

fn struck_over(beats: &[Beat], density: usize, dropout: f64, syncopation: f64) -> Vec<Onset> {
    let spans = lasting(beats);
    let unsounded = every_so_often(beats.len(), dropout);
    let sounding = unsounded.iter().filter(|dropped| !**dropped).count();
    let late = every_so_often(sounding, syncopation);
    let mut struck = 0;
    let mut onsets = Vec::new();

    for (index, beat) in beats.iter().enumerate() {
        if beat.is_downbeat {
            onsets.push(Onset {
                at: beat.at,
                voice: Voice::Accent,
            });
        }
        if unsounded[index] {
            continue;
        }

        let Some(span) = spans.get(index) else {
            continue;
        };

        let step = span.as_secs_f64() / density as f64;
        let shift = if late[struck] { step / 2.0 } else { 0.0 };
        struck += 1;
        for subdivision in 0..density {
            let at = on_the_sample_grid(beat.at.as_secs_f64() + step * subdivision as f64 + shift);
            if beat.is_downbeat && at == beat.at {
                continue;
            }
            onsets.push(Onset {
                at,
                voice: Voice::Tick,
            });
        }
    }

    onsets
}

fn every_so_often(count: usize, share: f64) -> Vec<bool> {
    let mut due = Vec::with_capacity(count);
    let mut accumulated = 0.0;

    for _ in 0..count {
        accumulated += share;
        let now = accumulated >= 1.0;
        if now {
            accumulated -= 1.0;
        }
        due.push(now);
    }

    due
}

const TAIL: Duration = Duration::from_millis(300);
const LONGEST: Duration = Duration::from_secs(40);
const SOFTEST_RISE: f64 = 0.020;
const SIGN_BIT: u8 = 0x80;
const ACCENT_FREQUENCY: f64 = 60.0;
const ACCENT_DECAY: f64 = 0.10;
const ACCENT_LEVEL: f64 = 0.85;
const TRANSIENT_DECAY: f64 = 0.006;
const TRANSIENT_LEVEL: f64 = 0.30;
const TICK_DECAY: f64 = 0.030;
const TICK_LEVEL: f64 = 0.55;
const RELEASE: Duration = Duration::from_millis(60);
const TONE_ATTACK: f64 = 0.005;
const TONE_DECAY: f64 = 0.020;
const TONE_LEVEL: f64 = 0.18;
const CONCERT_A: f64 = 440.0;
const CONCERT_A_PITCH: f64 = 69.0;
const SEMITONES: f64 = 12.0;

fn render(onsets: &[Onset], seed: u32, sharpness: f64) -> Vec<i8> {
    let length = onsets
        .iter()
        .map(|onset| onset.at + sounding(onset))
        .max()
        .unwrap_or_default();
    assert!(
        length <= LONGEST,
        "a fixture running {length:?} is longer than rendering will build"
    );
    let mut signal = vec![0.0; frames(length)];
    let mut noise = Noise::from(seed);

    for onset in onsets {
        let start = frames(onset.at);
        let held = frames(sounding(onset));
        for (offset, frame) in signal[start..].iter_mut().take(held).enumerate() {
            let elapsed = offset as f64 / f64::from(SAMPLE_RATE);
            *frame += match onset.voice {
                Voice::Accent => {
                    rising(elapsed, sharpness)
                        * (ACCENT_LEVEL
                            * (-elapsed / ACCENT_DECAY).exp()
                            * (std::f64::consts::TAU * ACCENT_FREQUENCY * elapsed).sin()
                            + TRANSIENT_LEVEL * (-elapsed / TRANSIENT_DECAY).exp() * noise.next())
                }
                Voice::Tick => {
                    rising(elapsed, sharpness)
                        * TICK_LEVEL
                        * (-elapsed / TICK_DECAY).exp()
                        * noise.next()
                }
                Voice::Tone { pitch, until } => {
                    let held = (until.saturating_sub(onset.at)).as_secs_f64();
                    TONE_LEVEL
                        * held_for(elapsed, held)
                        * (std::f64::consts::TAU * hertz(pitch) * elapsed).sin()
                }
            };
        }
    }

    signal
        .into_iter()
        .map(|frame| (frame.clamp(-1.0, 1.0) * f64::from(i8::MAX)) as i8)
        .collect()
}

fn sounding(onset: &Onset) -> Duration {
    match onset.voice {
        Voice::Tone { until, .. } => until.saturating_sub(onset.at) + RELEASE,
        Voice::Accent | Voice::Tick => TAIL,
    }
}

fn rising(elapsed: f64, sharpness: f64) -> f64 {
    let rise = SOFTEST_RISE * (1.0 - sharpness);
    if elapsed >= rise {
        return 1.0;
    }

    elapsed / rise
}

fn held_for(elapsed: f64, held: f64) -> f64 {
    let attack = (elapsed / TONE_ATTACK).min(1.0);
    let release = (-(elapsed - held).max(0.0) / TONE_DECAY).exp();

    attack * release
}

fn hertz(pitch: u8) -> f64 {
    CONCERT_A * ((f64::from(pitch) - CONCERT_A_PITCH) / SEMITONES).exp2()
}

fn entry(at: Duration, described: &str) -> String {
    format!("{:.6} {described}\n", at.as_secs_f64())
}

fn unsigned(sample: i8) -> u8 {
    (sample as u8) ^ SIGN_BIT
}

fn frames(at: Duration) -> usize {
    (at.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as usize
}

struct Noise(u32);

impl Noise {
    fn step(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;

        self.0
    }

    fn next(&mut self) -> f64 {
        f64::from(self.step()) / f64::from(u32::MAX) * 2.0 - 1.0
    }

    fn below(&mut self, bound: usize) -> usize {
        self.step() as usize % bound
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
