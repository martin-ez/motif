//! The shape of the loop, and the crossing that carries it to the screen.
//!
//! A summary stands in for samples the drawing thread may not touch, so the
//! facts worth stating are that it spans the whole loop however long that gets,
//! that a peak sharing a bucket survives rather than averaging away, and that
//! what is published reads back as it was written.
//!
//! The drawing is the other half. A region is rarely as wide as the buckets, so
//! the tests state both directions: narrower keeps the extreme in each column,
//! wider interpolates between the buckets either side.

use motif::looper::{Extremes, LoopWaveform, waveform_meter};

const BUCKETS: usize = LoopWaveform::BUCKETS;

fn summarising(samples: &[f32]) -> LoopWaveform {
    let mut waveform = LoopWaveform::EMPTY;
    waveform.take(0, samples.iter().copied());

    waveform
}

fn loud_at(frame: usize, frames: usize) -> LoopWaveform {
    let mut samples = vec![0.0; frames];
    samples[frame] = 1.0;

    summarising(&samples)
}

#[test]
fn an_empty_waveform_has_no_buckets() {
    assert!(LoopWaveform::EMPTY.buckets().is_empty());
}

#[test]
fn a_short_loop_gets_a_bucket_a_frame() {
    assert_eq!(summarising(&[0.5, -0.25, 0.75]).buckets().len(), 3);
}

#[test]
fn a_bucket_keeps_the_sample_that_reached_it() {
    assert_eq!(
        summarising(&[0.5, -0.25]).buckets()[1],
        Extremes {
            peak: 0.0,
            trough: -0.25,
        }
    );
}

#[test]
fn a_bucket_spans_the_frames_between_its_extremes() {
    assert_eq!(
        summarising(&[0.5, -0.25]).buckets()[0],
        Extremes {
            peak: 0.5,
            trough: 0.0,
        }
    );
}

#[test]
fn a_loop_past_the_bucket_count_folds_its_buckets() {
    assert_eq!(summarising(&vec![0.5; BUCKETS]).buckets().len(), BUCKETS);
    assert_eq!(
        summarising(&vec![0.5; BUCKETS + 1]).buckets().len(),
        BUCKETS / 2 + 1
    );
}

#[test]
fn a_peak_sharing_a_bucket_is_kept_rather_than_averaged() {
    assert_eq!(loud_at(3, BUCKETS * 2).buckets()[1].peak, 1.0);
}

#[test]
fn the_first_frame_of_a_long_loop_reaches_the_first_bucket() {
    let waveform = loud_at(0, BUCKETS * 4 + 1);

    assert_eq!(waveform.buckets()[0].peak, 1.0);
}

#[test]
fn the_last_frame_of_a_long_loop_reaches_the_last_bucket() {
    let frames = BUCKETS * 4 + 1;
    let waveform = loud_at(frames - 1, frames);

    assert_eq!(
        waveform.buckets().last().map(|bucket| bucket.peak),
        Some(1.0)
    );
}

#[test]
fn a_resweep_replaces_the_buckets_it_passes() {
    let mut waveform = summarising(&[1.0, 1.0, 1.0, 1.0]);
    waveform.take(0, [0.25, 0.25]);

    assert_eq!(waveform.buckets()[0].peak, 0.25);
    assert_eq!(waveform.buckets()[3].peak, 1.0);
}

#[test]
fn a_waveform_nobody_published_reads_as_empty() {
    assert_eq!(waveform_meter().1.read(), LoopWaveform::EMPTY);
}

#[test]
fn a_published_waveform_reads_back_whole() {
    let (mut writer, reader) = waveform_meter();
    let waveform = summarising(&[0.5, -0.25, 0.75]);
    writer.publish(&waveform);

    assert_eq!(reader.read(), waveform);
}

#[test]
fn a_long_published_waveform_reads_back_whole() {
    let (mut writer, reader) = waveform_meter();
    let waveform = loud_at(BUCKETS * 3, BUCKETS * 4);
    writer.publish(&waveform);

    assert_eq!(reader.read(), waveform);
}

#[test]
fn reading_a_waveform_leaves_it_published() {
    let (mut writer, reader) = waveform_meter();
    writer.publish(&summarising(&[0.5]));
    let _first = reader.read();

    assert_eq!(reader.read().buckets().len(), 1);
}

/// Two summaries whose every bucket differs, published against each other as
/// fast as two threads manage. Half of one and half of the other is a summary
/// neither thread ever held, and the only way to read one is to catch a publish
/// partway through.
#[test]
fn a_summary_read_against_a_publish_is_never_halves_of_two() {
    const ROUNDS: usize = 20_000;

    let (mut writer, reader) = waveform_meter();
    let loud = summarising(&vec![1.0; BUCKETS]);
    let quiet = summarising(&vec![0.25; BUCKETS]);
    writer.publish(&loud);

    let publishing = std::thread::spawn(move || {
        for round in 0..ROUNDS {
            writer.publish(if round % 2 == 0 { &loud } else { &quiet });
        }
    });

    for _ in 0..ROUNDS {
        let read = reader.read();
        let first = read.buckets()[0];
        assert!(
            read.buckets().iter().all(|bucket| *bucket == first),
            "a summary arrived as halves of two loops"
        );
    }

    publishing
        .join()
        .expect("the publishing thread ran to its end");
}

#[test]
fn an_empty_waveform_draws_blank_rows() {
    assert_eq!(LoopWaveform::EMPTY.drawn(3, 2), ["   ", "   "]);
}

#[test]
fn a_silent_loop_draws_nothing() {
    assert_eq!(summarising(&[0.0; 4]).drawn(4, 2), ["    ", "    "]);
}

#[test]
fn a_full_scale_loop_fills_every_row() {
    assert_eq!(
        summarising(&[1.0, -1.0, 1.0, -1.0]).drawn(2, 2),
        ["██", "██"]
    );
}

#[test]
fn a_half_scale_loop_fills_half_the_rows() {
    assert_eq!(summarising(&[0.5, -0.5]).drawn(1, 2), [" ", "█"]);
}

#[test]
fn a_region_narrower_than_the_buckets_keeps_the_extreme() {
    let waveform = summarising(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0]);

    assert_eq!(waveform.drawn(2, 1), [" █"]);
}

#[test]
fn a_region_wider_than_the_buckets_interpolates() {
    assert_eq!(summarising(&[0.0, 1.0]).drawn(3, 1), [" ▂▄"]);
}

#[test]
fn a_region_with_no_rows_draws_nothing() {
    assert!(summarising(&[1.0]).drawn(3, 0).is_empty());
}
