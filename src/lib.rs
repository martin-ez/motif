//! `motif` — a terminal groovebox that listens to you play, infers the musical
//! structure of what you played, and uses that structure to help you build a
//! song sketch.
//!
//! The crate is deliberately empty at this point. See `docs/brief.md` for the
//! design it is being built towards and `AGENTS.md` for the invariants any
//! contribution must hold.
//!
//! Four of those invariants are load-bearing enough to restate here, because
//! they constrain the shape of nearly every type in this crate:
//!
//! 1. **Analysis is retrospective, not causal.** The loop is captured first and
//!    analysed afterwards. There is a deadline, not a latency budget.
//! 2. **The audio callback is strictly real-time.** No allocation, no locking,
//!    no I/O, no unbounded loops on that thread.
//! 3. **The beat grid is an array of timestamps, not a BPM scalar.** A tempo
//!    number is a view derived from the grid, never the stored truth.
//! 4. **The UI renders through an abstraction**, not directly to a terminal.
//!    The terminal is today's backend; a small hardware screen is the goal.
