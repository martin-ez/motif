# motif

A groovebox for developing a single music loop into a whole song sketch.

In its most basic form it is a looper. What makes it a groovebox is what happens
to the loop once you have played it: the musical content gets worked out — the
tempo, the rhythm, the harmony, the notes — and that is what the instrument
builds on. Parts that accompany the loop, and variations that become new
sections.

It is a sketchpad, not a DAW. Everything is aimed at the distance between a
phrase you just played and something worth keeping.

The terminal is not the destination. The instrument this is aimed at is a
physical one — a compute module, a small display, a handful of controls — and
work on the device is deliberately deferred until the software has proved it is
worth putting in a box. So the terminal build mimics the thing it will become,
at the same display resolution and with the same controls:
`DeviceProfile::TARGET` is 66 by 20 cells, twelve buttons and one encoder, and
those are numbers the code already compiles against.

How much of this exists today is the next section, and it is the only part of
this file describing behaviour you can have.

## What runs today

`cargo run` opens a duplex stream on the default input and output, holds it for
the length of the run, and puts the looper on screen. Record opens the first
take, records again to layer over it, and drops back out of the layer; play
closes whatever is open and runs the loop; stop halts it keeping what was
recorded. Held with shift, play taps a pulse instead of starting one, record
mutes the input, stop takes the last layer back off and down empties the loop,
and the encoder moves the input gain a decibel a detent. The bottom row carries
the device's state and the input level, and `ctrl + c` ends the run.

The scene buttons pick a screen: `2` opens the audio settings, where the host,
the devices and their channels are chosen and the stream reopens on the choice,
and `1` returns to the looper.

The keyboard stands in for the panel the design is aimed at, twelve buttons and
one encoder: `z`, `x` and `c` are play, stop and record, `,` and `.` turn the
encoder, and an upper case letter is that button held with shift.

It wants a real terminal, since it switches one into raw mode and onto its
alternate screen. Run from an editor's output pane it will exit with `the
screen is not available` instead, which is the same path as a piped stdin.

This README describes what is here now. Work that has not happened lives in
[issues](https://github.com/martin-ez/motif/issues), not in prose.

## The bet

Online beat tracking is decent. Online *downbeat* tracking is not — roughly
47–53% F1 at the state of the art, against 75–80% for offline models on the same
task. Downbeat is what determines where bar one is, and therefore where a loop
should cut.

So the design analyses **retrospectively**: capture first, analyse afterwards.
That is not a compromise, it is the more accurate path, and it is worth about
25–30 percentage points on the metric that matters most.

It works because capture ends before analysis begins: there is a *deadline*, not
a latency budget, and a loop of a few bars leaves seconds of thinking time. Two
things a manual looper knows shrink the offline problem further — the take's
length is exact, since the player closed the loop, and its bar count is stated
rather than inferred.

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
scripts/gate.sh
```

That runs every check CI can fail a pull request on that a working tree can
answer, in the configuration CI uses, and reports all of them rather than
stopping at the first. The mutation sweep runs last and is the one check that
can fail a change the others approved; CI runs it on every pull request either
way.

Three assertions are left out because they belong to a pull request rather than
a tree: the title's length and shape, and the scan for tool attribution in the
body. [`AGENTS.md`](AGENTS.md) says what to do about those.

Debug builds compile dependencies at `opt-level = 2` and this crate at `1`.
Audio code is unusable at `opt-level = 0` — the DSP path underruns the callback
and timing measurements become noise.

## Repository layout

```
src/            the crate
tests/          every test lives here, so that tests reach only the public API
examples/       things to run against real hardware: devices, layout, fixtures
scripts/        the gate, the tracker, and the checks CI runs
AGENTS.md       how this project is built; read before contributing
```

## Development

This project is built agentically. [`AGENTS.md`](AGENTS.md) holds the working
agreement: documentation rules, test-first development, and trunk-based
branching. It applies to humans too.

Four consequences worth knowing before reading the code, because they look like
omissions otherwise:

- **There are no inline comments.** Doc comments on public items carry the
  explanation; anything else is expressed by naming things properly.
- **There are no `#[cfg(test)]` modules.** Every test lives in `tests/`, where it
  compiles as an external consumer of the crate. The compiler then guarantees
  that tests can only reach the public API, so implementation details stay free
  to change.
- **There is no roadmap in this repository.** The order of the work lives in the
  issue tracker, as a chain of epics each blocked by the one before it with
  every issue parented to one of them. That order is queryable, it closes itself
  as work lands, and nothing written here can quietly disagree with it.
- **Incomplete work sits behind Cargo features**, off by default, so branches
  stay short-lived and `main` stays green.

Work that is ready to pick up — open, unclaimed, with nothing open blocking it:

```sh
scripts/track.sh ready
```

Not `gh issue list --search`: the legacy search index ignores `is:blocked` and
returns blocked issues as though they were ready, with a 200 and no error.

## Licence

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option. This is the Rust ecosystem convention: downstream users pick
whichever fits their project.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work by you, as defined in the Apache-2.0 licence, shall
be dual-licensed as above, without any additional terms or conditions.
