//! Working out what a take was, once it has been played.
//!
//! Analysis is retrospective, so everything here is handed a whole take and
//! answers against a deadline rather than a latency budget. None of it runs on
//! the audio callback, and all of it is free to allocate.
//!
//! [`Envelope`] is the front end: where the take got louder. [`track`] is the
//! beat tracker over it, and [`Priors`] is what a manual looper knows that a
//! general beat tracker does not — the length the player chose, and how many
//! beats go to a bar.

mod envelope;
mod tracker;

pub use envelope::Envelope;
pub use tracker::{FASTEST, Priors, SLOWEST, Tracked, track};
