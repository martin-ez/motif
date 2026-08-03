# motif

A terminal groovebox that listens to you play, works out the musical structure of
what you played, and uses that understanding to help you build a song sketch.

You play. It captures a loop, correctly aligned to bar one. It tells you the
tempo, key, chord progression, and notes. You overdub, or let it generate parts
that fit.

It is a sketchpad, not a DAW. The terminal is a deliberate constraint: it keeps
UI design off the critical path and forces the interaction to stay simple.

> **Status: pre-alpha.** The crate is a skeleton — there is nothing to play yet.
> This repository currently holds the design, the invariants, and the scaffolding
> to build against them.

## The idea

Online beat tracking is decent. Online *downbeat* tracking is not — roughly
47–53% F1 at the state of the art, against 75–80% for offline models on the same
task. Downbeat is what determines where bar one is, and therefore where the loop
cuts.

So `motif` analyses **retrospectively**. The loop is captured first and analysed
afterwards. That is not a compromise; it is the more accurate path, worth about
25–30 percentage points on the metric that matters most.

This works because you set the loop length in bars up front. The system never
needs a causal model at all — it has a *deadline*, not a latency budget. At four
bars that is several seconds of thinking time, and once the loop closes, tempo
becomes exactly derivable (duration ÷ beat count) rather than estimated.

A rolling pre-roll buffer runs underneath all of it, which is what lets the loop
start move *earlier* than the moment you hit the button.

`docs/brief.md` has the full reasoning, the simplifying decisions, and the known
risks.

## Design invariants

Four decisions constrain nearly everything else. They are stated here because
changes that violate them tend to look like improvements:

1. **Analysis is retrospective, not causal** — a deadline, not a latency budget.
2. **The audio callback is strictly real-time** — no allocation, locking, or I/O.
3. **The beat grid is an array of timestamps, not a BPM scalar** — tempo is a
   derived view, never the stored truth.
4. **The UI renders through an abstraction, not to a terminal** — the terminal is
   today's backend; a small hardware screen is the goal.

Everything cross-compiles to `aarch64`.

## Scope

| | |
|---|---|
| **v1** | Passthrough, loop capture, beat grid, tempo, downbeat. One generated MIDI part. |
| **Later** | Chords, key, audio-to-MIDI, multi-layer overdubs, song sections. |
| **Deferred** | Source separation, rubato / warp maps, polyphonic multi-instrument input. |

## Building

Requires Rust 1.97.1, pinned in `rust-toolchain.toml` and installed automatically
by `rustup`.

```sh
cargo build
cargo test
cargo run
```

Before opening a pull request:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo check --target aarch64-unknown-linux-gnu
```

Debug builds compile dependencies at `opt-level = 2` and this crate at `1`. Audio
code is unusable at `opt-level = 0` — the DSP path underruns the callback and
timing measurements become noise.

## Repository layout

```
src/            the crate — currently a skeleton
docs/brief.md   the design, and why it is shaped this way
AGENTS.md       how this project is built; read before contributing
```

## Development

This project is built agentically. `AGENTS.md` holds the working agreement:
invariants, conventions, and how work is tracked.

Work is tracked in **GitHub Issues**, which is the single source of truth —
dependencies between tasks are modelled with issue relationships rather than
prose. To find work that is actually actionable:

```sh
gh issue list --search "is:open -is:blocked"
```

## Licence

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option. This is the Rust ecosystem convention: downstream users pick
whichever fits their project.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work by you, as defined in the Apache-2.0 licence, shall be
dual-licensed as above, without any additional terms or conditions.
