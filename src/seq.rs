//! Where the beats are, and where a frame falls among them.
//!
//! [`BeatGrid`] is the type design invariant 3 is about: the beats are an array
//! of timestamps, and a tempo is arithmetic over that array rather than a
//! number kept beside it. Storing a tempo and reconstructing positions from it
//! discards the timing detail the array holds, and makes a loop that drifts or
//! breathes unrepresentable — so there is nowhere here to put one.
//!
//! Beats are timestamped in frames of the sample clock, which is what captured
//! audio is timestamped against. Converting to seconds at the boundary of every
//! consumer is where rounding creeps in, so the grid does not offer one.

mod grid;

pub use grid::{BeatGrid, Position};
