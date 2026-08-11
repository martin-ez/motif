//! Working out what a take was, once it has been played.
//!
//! Analysis is retrospective, so everything here is handed a whole take and
//! answers against a deadline rather than a latency budget. None of it runs on
//! the audio callback, and all of it is free to allocate.
//!
//! [`Envelope`] is the onset front end: where the take got louder. [`track`] is
//! the beat tracker over it, and [`Priors`] is what a manual looper knows that
//! a general beat tracker does not — the length the player chose, and how many
//! beats go to a bar. [`Transform`] is the spectral one: a window of samples as
//! the magnitude of each frequency in it.

mod envelope;
mod spectrum;
mod tracker;

pub use envelope::Envelope;
pub use spectrum::Transform;
pub use tracker::{FASTEST, Priors, SLOWEST, Tracked, track};
