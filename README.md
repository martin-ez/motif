# motif

A terminal groovebox for capturing a loop of what you play, inferring its
musical structure, and using that structure to help build a song sketch.

It is meant to be a sketchpad, not a DAW. The terminal is a deliberate
constraint: it keeps UI design off the critical path and forces the interaction
to stay simple.

> **Status: nothing works yet.** The crate is a skeleton — `cargo run` prints a
> line and exits. What exists today is the design, the invariants, and the
> tooling that enforces them.
>
> This README describes what is here now. Work that has not happened lives in
> [issues](https://github.com/martin-ez/motif/issues), not in prose — see
> `AGENTS.md` rule 1.7.

## The bet

Online beat tracking is decent. Online *downbeat* tracking is not — roughly
47–53% F1 at the state of the art, against 75–80% for offline models on the same
task. Downbeat is what determines where bar one is, and therefore where a loop
should cut.

So the design analyses **retrospectively**: capture first, analyse afterwards.
That is not a compromise, it is the more accurate path, and it is worth about
25–30 percentage points on the metric that matters most.

It works because the loop length is set in bars up front, which means no causal
model is needed at all — there is a *deadline*, not a latency budget. At four
bars that is several seconds of thinking time, and once the loop closes, tempo
becomes exactly derivable (duration ÷ beat count) rather than estimated.

[`docs/README.md`](docs/README.md) is the founding brief: the full argument, the
simplifying decisions, and the known risks. Read it as intent, not as a
description of working software.

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

## Building

Requires Rust 1.97.1, pinned in `rust-toolchain.toml` and installed
automatically by `rustup`.

```sh
cargo build
cargo test
cargo run
```

Before opening a pull request:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
scripts/check-style.sh
cargo check --target aarch64-unknown-linux-gnu
```

Debug builds compile dependencies at `opt-level = 2` and this crate at `1`.
Audio code is unusable at `opt-level = 0` — the DSP path underruns the callback
and timing measurements become noise.

## Repository layout

```
src/            the crate
tests/          every test lives here, by design (see below)
docs/README.md  the founding brief
scripts/        house-style checks that rustc and clippy cannot express
AGENTS.md       how this project is built; read before contributing
```

## Development

This project is built agentically. [`AGENTS.md`](AGENTS.md) holds the working
agreement — documentation rules, test-first development, trunk-based branching,
and how each rule is enforced. It applies to humans too.

Four consequences worth knowing before reading the code, because they look like
omissions otherwise:

- **There are no inline comments.** Doc comments on public items carry the
  explanation; anything else is expressed by naming things properly.
- **There are no `#[cfg(test)]` modules.** Every test lives in `tests/`, where it
  compiles as an external consumer of the crate. The compiler then guarantees
  that tests can only reach the public API, so implementation details stay free
  to change.
- **There is no roadmap in this repository.** Planned work is in the issue
  tracker, where it can block and be blocked by other work.
- **Incomplete work sits behind Cargo features**, off by default, so branches
  stay short-lived and `main` stays green.

Work that is ready to pick up — open, with nothing open blocking it:

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
for inclusion in this work by you, as defined in the Apache-2.0 licence, shall
be dual-licensed as above, without any additional terms or conditions.
