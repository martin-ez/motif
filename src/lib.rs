//! `motif` — a terminal groovebox for capturing a loop of what you play and
//! inferring its musical structure.
//!
//! `AGENTS.md` holds the design invariants that constrain what belongs here.

mod closed_set;

pub mod audio;
pub mod device;
pub mod fixtures;
pub mod looper;
pub mod monitor;
pub mod seq;
pub mod ui;
