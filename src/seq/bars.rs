//! How a take divides, as the player states it rather than as anything infers
//! it.
//!
//! The two halves are one value because a count is worth nothing without the
//! meter it is counted in, and because what carries it to the audio callback
//! and back off it carries a fixed number of bits.

use std::num::NonZeroU16;

const HALF: u32 = 16;

/// How a take divides: the bars a player says it runs, and the beats in each.
///
/// Stated, never inferred and never assumed. Over takes whose bar count
/// varies, a stated count places downbeats at 0.91 F1 where the meter alone
/// reaches 0.71, and a guessed four bars reaches 0.34 — well below not counting
/// at all. So a take nobody counted carries no count rather than a likely one.
///
/// Both halves travel together because neither means anything alone: a count of
/// bars says nothing about beats until a bar has a length.
///
/// ```
/// use motif::seq::Bars;
///
/// let bars = Bars::of(4, 3).expect("four bars of three beats is a count");
///
/// assert_eq!(bars.count(), 4);
/// assert_eq!(bars.beats_each(), 3);
/// assert_eq!(Bars::of(4, 0), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bars {
    count: NonZeroU16,
    beats_each: NonZeroU16,
}

impl Bars {
    /// The most bars a count states, and the most beats it puts in one of them.
    ///
    /// Half of a command payload each, which is the range those bits hold
    /// rather than a limit on anything anyone plays.
    pub const MOST: usize = u16::MAX as usize;

    /// A take of `count` bars, each of `beats_each` beats, where that is a
    /// count at all.
    ///
    /// Neither half may be zero — a take of no bars is not a take, and a bar of
    /// no beats is not a bar — nor past [`MOST`](Self::MOST).
    pub fn of(count: usize, beats_each: usize) -> Option<Self> {
        Some(Self {
            count: NonZeroU16::new(u16::try_from(count).ok()?)?,
            beats_each: NonZeroU16::new(u16::try_from(beats_each).ok()?)?,
        })
    }

    /// How many bars the take runs.
    pub const fn count(self) -> usize {
        self.count.get() as usize
    }

    /// How many beats go to one of its bars.
    pub const fn beats_each(self) -> usize {
        self.beats_each.get() as usize
    }

    pub(crate) const fn to_bits(self) -> u32 {
        ((self.count.get() as u32) << HALF) | self.beats_each.get() as u32
    }

    pub(crate) fn from_bits(bits: u32) -> Option<Self> {
        Self::of((bits >> HALF) as usize, (bits & u16::MAX as u32) as usize)
    }
}
