# AGENTS.md

How this project is built. It applies to agents and humans alike.

`motif` is a terminal groovebox: it captures a loop of what you play, infers its
musical structure, and uses that to help build a song sketch. The crate is a
skeleton and nothing works yet — what is ready to pick up is in the issue
tracker, not here.

## Commands

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-style.sh
cargo check --target aarch64-unknown-linux-gnu
```

Run all of them before opening a pull request.

## Design invariants

Load-bearing. A change that breaks one is wrong even if it compiles, passes
tests, and looks tidier.

1. **Analysis is retrospective, not causal.** Capture first, analyse afterwards:
   offline downbeat tracking is ~25–30 points more accurate than online. There is
   a *deadline*, not a latency budget. Do not add a causal or streaming analyser
   to "reduce latency" — that trades away the project's central advantage.

2. **The audio callback is strictly real-time.** On that thread: no allocation,
   locking, I/O, syscalls, unbounded loops, or panicking paths. Talk to the rest
   of the system over lock-free channels and pre-allocated buffers, and allocate
   at setup.

3. **The beat grid is an array of timestamps, not a BPM scalar.** Tempo is a view
   derived from the grid, never the stored truth: storing a number and
   reconstructing positions from it discards timing detail and makes rubato
   unrepresentable. The estimator may assume steady-ish tempo; the data model may
   not.

4. **The UI renders through an abstraction, not to a terminal.** The terminal is
   today's backend, a small hardware screen is the goal. No terminal-specific
   calls outside the UI backend layer.

Corollary: **everything cross-compiles to aarch64.** Check before adding a
dependency.

---

## 1. Documentation

Documentation is a first-class citizen here, not a step at the end.

**1.1 Clear and concise.** Say the thing and stop. Prose that restates a
signature in English is worse than nothing: it has to be maintained, and it will
go stale.

**1.2 Prefer in-code documentation over documents.** A doc comment is reviewed in
the same diff as the code it describes and cannot drift unnoticed; a standalone
file can. If it explains how something works, it is a doc comment.

**1.3 Only public members have doc comments, and every public member has one.**
Public means it appears in `cargo doc`. Private items are not part of any
contract, and a doc comment on one is clutter that outlives the code.

**1.4 Inline comments are not allowed.** Name the value, or extract the step into
a function whose name is the sentence you were going to write.

> One exception in substance, not in form: a threshold from a paper, a constant
> measured against real audio. That provenance is not derivable from the code, so
> put it in the doc comment of the public item that uses it — never in a `//`,
> never on a private constant.

**1.5 Tests are documentation.** A reader should learn what a type does from its
tests. Name them as sentences — `tempo_is_derived_from_beat_count`, not
`test_tempo_2` — and prefer several small tests stating one fact each. Doc
examples run under `cargo test`, so use them wherever the calling pattern is not
obvious.

**1.6 The only documentation outside the code is a per-folder `README.md`,**
describing that folder. No `docs/architecture.md`, no decision-record directory,
no folder holding nothing but prose. Documentation trees rot from the leaves
inward, where nothing links.

Exempt, being the project's contract rather than documentation of it: `README.md`,
`AGENTS.md`, `CONTRIBUTING.md`, and templates under `.github/`. `CLAUDE.md` is a
symlink to this file.

**1.7 Never document a feature that does not exist.** No "coming soon", no
roadmap, no doc comment on a stub describing what it will become. Unbuilt work
belongs in a GitHub issue, where it is queryable, can block other work, and gets
closed when it lands. Aspirational prose does none of that: no test contradicts
it, and it becomes a lie the moment the plan changes.

A `README.md` may state what the project is *for* and the constraints it is built
under; it may not describe behaviour a user cannot get today. There are no
exemptions — not for a founding brief, a design document, or a plan. Import what
is still true into the code or the `README.md`, and put the rest in issues.

## 2. Development

**2. All code is developed test-first.** Write the test, watch it fail for the
right reason, then write the code that makes it pass.

**2.1 Only public members are tested,** so implementation details stay free to
change. Enforced by construction: all tests live in `tests/`, where they compile
as an external consumer and the compiler makes private items unreachable. A test
that needs a private item means either the item should be public, or you are
testing the wrong thing.

**2.2 Make them go red, then green.** A test that has never failed has never
demonstrated that it can. Nothing in a diff reveals the order, so this one rests
on you — but `cargo-mutants` checks its purpose, and tests written afterwards to
mirror an implementation tend to die on it.

**Accuracy claims need a measurement.** "This improves downbeat detection" is not
reviewable; a number against a fixture set is. Keep fixtures in `tests/fixtures`,
a few hundred KB at most.

## 3. Trunk-based development

**3.1 Branches are short-lived** — hours or a day. If a change cannot be finished
that fast it is several changes: split it and merge the first.

**3.2 Merge small and often.** A large pull request gets approved rather than
read.

**3.3 `main` is always green.** Never commit to `main`, never force-push, never
merge your own pull request. A change that breaks `main` is reverted first and
diagnosed second. The ruleset has no bypass, so a rejected push means find
another route, not try harder.

**3.4 Incomplete work sits behind a Cargo feature,** named for the capability
rather than the ticket. This is what makes 3.1 and 3.2 possible: unfinished work
merges safely because it is unreachable. `TODO` and `FIXME` markers are rejected
— incomplete work goes behind a flag and into an issue, where it is visible and
can block other work.

## 4. Pull requests

**4.1 Be concise and descriptive.** Give the reader what they need to trust the
change, and nothing else.

**4.2 The title is the summary.** It must be clear from the title alone what the
change does; it is what lands in `main`'s history after a squash merge. Use
`type(scope): summary`.

**4.3 The body says what changed and why, not how.** Do not restate the diff,
walk through the implementation, or explain a function a reader can open. Do not
add a verification or test section — CI reports what passed, and prose repeating
it is a claim rather than evidence. The shape is context, then `### Changes`; a
further section is allowed, but should be rare.

**4.4 Titles are at most 50 characters.** If the change will not fit, it is
usually two changes.

**4.5 Never add a co-author.** No `Co-authored-by:` trailer on any commit, no tool
attribution in any body. Agents especially: the tool that produced a change is
not a fact about the change.

## Task tracking

**GitHub Issues is the single source of truth.** Not a markdown TODO list, not a
second tracker — dual tracking is the main way a project like this loses track of
itself.

**Reach it through `scripts/track.sh`, never `gh` directly.**

```sh
scripts/track.sh ready            # what can be started right now
scripts/track.sh show 7           # one issue in full, before you write any code
scripts/track.sh claim 7          # exit 2: someone else has it, take the next row
scripts/track.sh done 7 -m "..."  # closes it, prints what that unblocked
scripts/track.sh --help           # add, dep, note, release, blocked, graph, doctor
```

Take the top row of `ready`; it sorts by how much each item unblocks, so the top
row is the one that frees the most work. Any non-zero exit other than 2 is fatal
— surface stderr and stop. Release anything you will not finish.

> This is a correctness rule, not a preference. The legacy search index — raw
> GraphQL `search(type: ISSUE)`, or REST without `advanced_search=true` —
> silently ignores `is:blocked` and returns blocked issues as ready, with a 200
> and no error. The script derives readiness from each issue's `blockedBy`
> payload instead, which makes that failure unreachable rather than merely
> documented, and read-your-writes consistent where the index lags by seconds.

It also spaces writes against the 80/minute and 500/hour account limits, and
holds a lock across both the read and the write in `claim`, so two agents cannot
take the same issue.

Issue types and fields are organisation-only, so metadata is labels: `area:`,
`kind:` and `size:`, all three required by `add`. `ready` offers only what
`claim` will accept — a `size:l` issue is listed under `SPLIT:` instead, and is
split with `add --parent <n>`.

## Working agreement

- **One task per session, one pull request per task.** Scope creep is the most
  expensive thing an agent can do here.
- **Branch from `main`** and open pull requests as drafts.
- **New dependencies need a reason**: what it does, why not hand-rolled, its
  licence, and that it cross-compiles.
- **No `unwrap()` or `expect()` where failure is possible at runtime.** Setup and
  tests are fine; the audio path and user input are not.
- **Match the surrounding code.** Its naming and idiom beat personal preference.
- **Report honestly.** If tests fail, say so and paste the output. If part of the
  task was skipped, say which and why.
- **State assumptions rather than blocking.** Pick the reading a careful
  colleague would, write it down in the pull request, and keep going.

## Commits

Conventional Commits, imperative mood, subject at most 50 characters, no
trailers:

```
feat(analysis): derive tempo from the beat grid
fix(audio): drop allocation from capture callback
```

The body is optional. Use it for a decision a future reader would otherwise have
to reconstruct, not to describe the diff.
