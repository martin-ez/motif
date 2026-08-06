//! Deciding what a playback block does to hold the slack a boundary was built
//! with.
//!
//! The rule is exercised on its own here, with no ring and no callback: it is
//! arithmetic over how many frames are waiting and how many the block wants, so
//! everything a stream would take minutes to show can be stated in one call.
//!
//! The facts worth stating are that a ring near its target is left alone, that
//! one running long gives frames up and one running short is given frames, that
//! a correction is capped per block and runs on until the target is reached
//! rather than stopping at the edge of the band, that a ring too dry to fill the
//! block is not padded on top of the shortfall, and that what the trim did is
//! readable from the other end.

use motif::audio::{Slack, SlackReader, SlackTrim, Trim, slack_hold};

const BLOCK: usize = 4;
const SLACK: usize = 4;

/// The occupancy a block of [`BLOCK`] frames should begin with, against a
/// boundary built with [`SLACK`] frames of slack.
const TARGET: usize = SLACK + BLOCK;

/// A block long enough that a correction is several frames rather than one, so
/// that what caps a correction is visible at all.
const WIDE: usize = 64;

/// The occupancy a block of [`WIDE`] frames should begin with.
const WIDE_TARGET: usize = WIDE + WIDE;

fn holding() -> (SlackTrim, SlackReader) {
    slack_hold(SLACK, BLOCK)
}

/// A wide hold already past its band, so that what a correction is capped at is
/// what the test reads rather than whether one has started.
fn correcting() -> SlackTrim {
    let (mut trim, _reader) = slack_hold(WIDE, WIDE);
    trim.trim(0, WIDE);
    trim
}

fn trimmed(available: usize) -> Trim {
    holding().0.trim(available, BLOCK)
}

fn after(available: usize) -> Slack {
    let (mut trim, reader) = holding();
    trim.trim(available, BLOCK);
    reader.read()
}

#[test]
fn a_ring_at_its_target_is_left_alone() {
    assert_eq!(trimmed(TARGET), Trim::Steady);
}

#[test]
fn a_ring_a_little_long_is_left_alone() {
    assert_eq!(trimmed(TARGET + BLOCK / 2), Trim::Steady);
}

#[test]
fn a_ring_a_little_short_is_left_alone() {
    assert_eq!(trimmed(TARGET - BLOCK / 2), Trim::Steady);
}

#[test]
fn a_ring_running_long_gives_frames_up() {
    assert_eq!(trimmed(TARGET + BLOCK / 2 + 1), Trim::Drop(1));
}

#[test]
fn a_ring_running_short_is_given_frames() {
    assert_eq!(trimmed(TARGET - BLOCK / 2 - 1), Trim::Insert(1));
}

#[test]
fn a_ring_too_dry_to_fill_the_block_is_not_padded() {
    assert_eq!(trimmed(BLOCK - 2), Trim::Steady);
}

#[test]
fn a_correction_is_capped_at_a_fraction_of_the_block() {
    let (mut trim, _reader) = slack_hold(WIDE, WIDE);

    assert_eq!(trim.trim(1_000, WIDE), Trim::Drop(2));
}

#[test]
fn a_wide_ring_too_dry_to_fill_the_block_is_not_padded() {
    assert_eq!(correcting().trim(WIDE - 24, WIDE), Trim::Steady);
}

#[test]
fn an_excess_within_the_cap_is_given_up_whole() {
    assert_eq!(correcting().trim(WIDE_TARGET + 2, WIDE), Trim::Drop(2));
}

#[test]
fn an_excess_of_one_frame_costs_one_frame() {
    assert_eq!(correcting().trim(WIDE_TARGET + 1, WIDE), Trim::Drop(1));
}

#[test]
fn a_shortfall_of_one_frame_is_made_up_with_one_frame() {
    assert_eq!(correcting().trim(WIDE_TARGET - 1, WIDE), Trim::Insert(1));
}

#[test]
fn a_padded_block_always_keeps_a_frame_to_hold() {
    let (mut trim, _reader) = slack_hold(256, 256);

    assert_eq!(trim.trim(1, 4), Trim::Insert(3));
}

#[test]
fn a_ring_the_block_exactly_drains_is_padded() {
    assert_eq!(trimmed(BLOCK - 1), Trim::Insert(1));
}

#[test]
fn a_small_block_is_corrected_a_frame_at_a_time() {
    assert_eq!(trimmed(1_000), Trim::Drop(1));
}

#[test]
fn a_drop_never_takes_a_frame_the_block_wanted() {
    for available in 0..256 {
        let (mut trim, _reader) = slack_hold(0, 64);

        if let Trim::Drop(dropped) = trim.trim(available, 64) {
            assert!(
                available - dropped >= 64,
                "dropping {dropped} of {available} left the block short"
            );
        }
    }
}

#[test]
fn a_correction_runs_on_until_the_target_is_reached() {
    let (mut trim, _reader) = holding();
    trim.trim(TARGET + BLOCK / 2 + 1, BLOCK);

    assert_eq!(trim.trim(TARGET + 1, BLOCK), Trim::Drop(1));
}

#[test]
fn a_correction_stops_once_the_target_is_reached() {
    let (mut trim, _reader) = holding();
    trim.trim(TARGET + BLOCK / 2 + 1, BLOCK);
    trim.trim(TARGET, BLOCK);

    assert_eq!(trim.trim(TARGET + 1, BLOCK), Trim::Steady);
}

#[test]
fn nothing_is_spent_before_the_first_block() {
    let (_trim, reader) = holding();

    assert_eq!(reader.read(), Slack::NONE);
}

#[test]
fn the_held_slack_is_what_the_block_leaves_behind() {
    assert_eq!(after(TARGET).held, SLACK);
}

#[test]
fn the_held_slack_counts_the_frames_the_trim_took() {
    assert_eq!(after(TARGET + BLOCK / 2 + 1).held, SLACK + BLOCK / 2);
}

#[test]
fn the_held_slack_counts_the_frames_the_trim_added() {
    assert_eq!(after(TARGET - BLOCK / 2 - 1).held, SLACK - BLOCK / 2);
}

#[test]
fn a_ring_the_block_drained_holds_nothing() {
    assert_eq!(after(BLOCK - 2).held, 0);
}

#[test]
fn dropped_frames_accumulate() {
    let (mut trim, reader) = holding();
    trim.trim(TARGET + BLOCK, BLOCK);
    trim.trim(TARGET + BLOCK, BLOCK);

    assert_eq!(reader.read().dropped, 2);
}

#[test]
fn inserted_frames_accumulate() {
    let (mut trim, reader) = holding();
    trim.trim(TARGET - BLOCK, BLOCK);
    trim.trim(TARGET - BLOCK, BLOCK);

    assert_eq!(reader.read().inserted, 2);
}

#[test]
fn a_steady_block_spends_nothing() {
    let steady = after(TARGET);

    assert_eq!((steady.dropped, steady.inserted), (0, 0));
}
