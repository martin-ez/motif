//! Working out what a take was, once it has been played.
//!
//! Analysis is retrospective, so everything here is handed a whole take and
//! answers against a deadline rather than a latency budget. None of it runs on
//! the audio callback, and all of it is free to allocate.
//!
//! [`Envelope`] is the front end: where the take got louder.

mod envelope;

pub use envelope::Envelope;
