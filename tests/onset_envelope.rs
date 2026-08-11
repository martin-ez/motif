//! The onset envelope a beat tracker reads: where a take got louder, and by
//! how much.
//!
//! The facts worth stating are that silence carries nothing, that a click is
//! strongest where it starts rather than where it is loudest, that a held tone
//! is an onset once rather than for as long as it sounds, that the hop is a
//! whole number of frames of the clock the samples came from, and that the
//! envelope covers the audio it was taken over and nothing past it.

use motif::analysis::Envelope;
use std::time::Duration;

const SAMPLE_RATE: u32 = 8_000;
const BURST: Duration = Duration::from_millis(30);
const DECAY: f32 = 0.01;
const LOUD: f32 = 0.8;
const QUIET: f32 = 0.1;
const A_SECOND: Duration = Duration::from_secs(1);
const HALFWAY: Duration = Duration::from_millis(500);

fn frames(at: Duration) -> usize {
    (at.as_secs_f64() * f64::from(SAMPLE_RATE)).round() as usize
}

fn silence(length: Duration) -> Vec<f32> {
    vec![0.0; frames(length)]
}

fn struck(signal: &mut [f32], at: Duration, level: f32) {
    let alternating = |offset: usize| if offset.is_multiple_of(2) { 1.0 } else { -1.0 };

    for (offset, frame) in signal[frames(at)..]
        .iter_mut()
        .take(frames(BURST))
        .enumerate()
    {
        let elapsed = offset as f32 / SAMPLE_RATE as f32;
        *frame = level * (-elapsed / DECAY).exp() * alternating(offset);
    }
}

fn held(signal: &mut [f32], from: Duration) {
    for (offset, frame) in signal[frames(from)..].iter_mut().enumerate() {
        let elapsed = offset as f32 / SAMPLE_RATE as f32;
        *frame = LOUD * (std::f32::consts::TAU * 220.0 * elapsed).sin();
    }
}

fn one_click(at: Duration, level: f32) -> Vec<f32> {
    let mut signal = silence(A_SECOND);
    struck(&mut signal, at, level);

    signal
}

fn envelope_of(signal: &[f32]) -> Envelope {
    Envelope::of(signal.iter().copied(), SAMPLE_RATE)
}

fn strongest(envelope: &Envelope) -> Duration {
    let (frame, _) = envelope
        .strength()
        .iter()
        .enumerate()
        .max_by(|(_, one), (_, other)| one.total_cmp(other))
        .expect("the envelope has frames");

    envelope.hop() * frame as u32
}

#[test]
fn silence_carries_no_onset_strength() {
    let envelope = envelope_of(&silence(A_SECOND));

    assert!(
        envelope.strength().iter().all(|strength| *strength == 0.0),
        "silence rose somewhere: {:?}",
        envelope.strength()
    );
}

#[test]
fn a_click_is_strongest_where_it_starts() {
    let envelope = envelope_of(&one_click(HALFWAY, LOUD));

    assert!(
        strongest(&envelope).abs_diff(HALFWAY) <= envelope.hop(),
        "the strongest frame fell at {:?} rather than {HALFWAY:?}",
        strongest(&envelope)
    );
}

#[test]
fn a_louder_click_is_stronger_than_a_quieter_one() {
    let loud = envelope_of(&one_click(HALFWAY, LOUD));
    let quiet = envelope_of(&one_click(HALFWAY, QUIET));

    assert!(
        loud.at(HALFWAY) > quiet.at(HALFWAY),
        "{} did not beat {}",
        loud.at(HALFWAY),
        quiet.at(HALFWAY)
    );
}

#[test]
fn a_held_tone_is_an_onset_where_it_begins_and_not_after() {
    let mut signal = silence(A_SECOND);
    held(&mut signal, HALFWAY);
    let envelope = envelope_of(&signal);
    let sustained = HALFWAY + BURST + BURST;

    assert!(envelope.at(HALFWAY) > 0.0);
    assert!(
        envelope.at(sustained) < envelope.at(HALFWAY) / 10.0,
        "a tone still sounding read as an onset: {}",
        envelope.at(sustained)
    );
}

#[test]
fn the_hop_is_a_whole_number_of_frames_of_the_clock_the_samples_came_from() {
    let envelope = envelope_of(&silence(A_SECOND));
    let hop = envelope.hop().as_secs_f64() * f64::from(SAMPLE_RATE);

    assert_eq!(hop, hop.round(), "the hop is {hop} frames");
    assert_eq!(envelope.hop(), Envelope::HOP);
}

#[test]
fn an_envelope_covers_the_audio_it_was_taken_over() {
    let envelope = envelope_of(&silence(A_SECOND));

    assert!(
        envelope.span().abs_diff(A_SECOND) <= envelope.hop(),
        "an envelope of {A_SECOND:?} spans {:?}",
        envelope.span()
    );
}

#[test]
fn strength_past_the_end_of_the_audio_is_none() {
    let envelope = envelope_of(&one_click(HALFWAY, LOUD));

    assert_eq!(envelope.at(A_SECOND + BURST), 0.0);
}

#[test]
fn strength_at_a_moment_is_the_frame_covering_it() {
    let envelope = envelope_of(&one_click(HALFWAY, LOUD));
    let frame = frames(HALFWAY) / frames(envelope.hop());

    assert_eq!(envelope.at(HALFWAY), envelope.strength()[frame]);
    assert_eq!(
        envelope.at(HALFWAY + envelope.hop() / 3),
        envelope.at(HALFWAY)
    );
}

#[test]
fn an_envelope_of_nothing_has_no_frames() {
    let envelope = envelope_of(&[]);

    assert_eq!(envelope.strength(), &[] as &[f32]);
    assert_eq!(envelope.span(), Duration::ZERO);
    assert_eq!(envelope.at(Duration::ZERO), 0.0);
}

#[test]
fn a_hop_carries_the_mean_energy_of_its_samples_rather_than_the_total() {
    const HELD_AT: f32 = 0.5;
    const A_TENTH: u32 = 10;
    const TWICE_AS_OFTEN: u32 = SAMPLE_RATE * 2;

    let first_rise = |rate: u32| {
        let held = vec![HELD_AT; (rate / A_TENTH) as usize];
        let envelope = Envelope::of(held, rate);

        envelope.strength()[0]
    };

    assert_eq!(
        first_rise(SAMPLE_RATE),
        first_rise(TWICE_AS_OFTEN),
        "the same tone read at two rates rose by different amounts"
    );
}

#[test]
fn the_part_hop_a_take_ends_on_is_measured_like_a_whole_one() {
    const HELD_AT: f32 = 0.5;
    const WHOLE_HOPS: usize = 8;

    let per_hop = frames(Envelope::HOP);
    let held = vec![HELD_AT; per_hop * WHOLE_HOPS + per_hop / 2];
    let envelope = envelope_of(&held);

    assert_eq!(envelope.strength().len(), WHOLE_HOPS + 1);
    assert_eq!(envelope.strength().last(), Some(&0.0));
}
