# AGENTS.md

Instructions for any coding agent working in this repository. Humans should read
it too — it is the short version of how this project is built.

## What this is

`motif` is a terminal groovebox: it listens to you play, captures a loop aligned
to bar one, infers the musical structure of what you played, and uses that
structure to help build a song sketch.

**Read `docs/brief.md` before your first change.** It is short, and it explains
*why* the architecture is shaped the way it is. Most bad changes to this codebase
come from not knowing the reasoning in that document.

Status: early. The crate is a skeleton. v1 scope is passthrough, loop capture,
beat grid, tempo, downbeat, and one generated MIDI part.

## Commands

```sh
cargo build                 # build
cargo test                  # test
cargo fmt --all             # format (run before committing)
cargo clippy --all-targets --all-features -- -D warnings
cargo check --target aarch64-unknown-linux-gnu   # cross-compile guard
```

CI runs fmt, clippy with `-D warnings`, tests on Linux and macOS, and the
aarch64 check. Run them locally before opening a PR — a red CI on a draft PR is
noise for everyone.

## Invariants

These four are load-bearing. A change that breaks one is wrong even if it
compiles, passes tests, and looks tidier.

1. **Analysis is retrospective, not causal.** The loop is captured first and
   analysed afterwards, because offline downbeat tracking is ~25–30 points more
   accurate than online. There is a *deadline*, not a latency budget. Do not
   introduce a causal/streaming analyser to "reduce latency" — that trades away
   the project's central advantage.

2. **The audio callback is strictly real-time.** On that thread: no heap
   allocation, no locking, no I/O, no syscalls, no unbounded loops, no panicking
   paths. Communicate with the rest of the system over lock-free channels and
   pre-allocated buffers. If you need to allocate, do it at setup.

3. **The beat grid is an array of timestamps, not a BPM scalar.** Tempo is a
   *view* derived from the grid, never the stored truth. Storing a BPM number
   and reconstructing beat positions from it silently discards timing detail and
   makes rubato unrepresentable. The estimator may assume steady-ish tempo; the
   data model may not.

4. **The UI renders through an abstraction, not directly to a terminal.** The
   terminal is today's backend; a small hardware screen is the goal. No
   terminal-specific calls outside the UI backend layer.

Corollary that follows from 1–4: **everything cross-compiles to aarch64.** Check
before adding a dependency.

## Conventions

- **Match the surrounding code.** Its naming, comment density, and idiom win over
  personal preference.
- **Comments explain why, not what.** The DSP in this project is full of
  non-obvious constants and thresholds; every one of them needs a sentence about
  where it came from.
- **No `unwrap()`/`expect()` on paths that can fail at runtime.** Tests and
  setup-time invariants are fine; the audio path and user input are not.
- **New dependencies need a reason in the PR description**: what it does, why not
  hand-rolled, its license, and that it cross-compiles.
- **Tests:** unit tests next to the code, integration tests in `tests/`. Analysis
  code needs fixtures — commit small audio/MIDI fixtures under `tests/fixtures`
  and keep them under a few hundred KB.
- **Accuracy claims need a measurement.** "This improves downbeat detection" is
  not reviewable; a number against a fixture set is.

## Task tracking

**GitHub Issues is the single source of truth** for work done and to be done.
Not a markdown TODO list, not a second tracker. Dual tracking is the main way
this kind of project loses track of itself.

Find work that is actually actionable — open, with no open blockers:

```sh
gh issue list --search "is:open -is:blocked"
```

> This must go through `gh issue list --search`, which queries GitHub's
> *advanced* search index. The legacy index (raw GraphQL `search(type: ISSUE)`)
> silently ignores dependency qualifiers and returns wrong results with no
> error. If you are writing a raw query, you want `ISSUE_ADVANCED` in GraphQL or
> `?advanced_search=true` in REST.

See what is blocked, and on what:

```sh
gh issue list --state open --json number,title,blockedBy \
  --jq '.[] | select([.blockedBy.nodes[] | select(.state=="OPEN")] | length > 0)
        | "#\(.number) \(.title)  <- waiting on \([.blockedBy.nodes[]|select(.state=="OPEN")|"#\(.number)"]|join(", "))"'
```

Link work as you create it:

```sh
gh issue create --title "..." --body-file spec.md --parent 12 --blocked-by 8,9
gh issue edit 14 --add-blocked-by 8 --add-sub-issue 15
```

Notes:
- Issue *types* and issue *fields* are organisation-only features and are
  unavailable on this repo. Use **labels** for metadata.
- Writes are subject to a secondary rate limit of 80/minute and 500/hour across
  the whole account. When creating many issues, space them by at least a second.
- Post progress with `gh issue comment N --edit-last --create-if-none --body ...`
  so a long task leaves one updating comment rather than a wall of them.

## Working agreement

- **One task per session, one PR per task.** Scope creep in a PR is the most
  expensive thing an agent can do here.
- **Branch from `main`.** Never commit to `main`, never force-push, never merge
  your own PR.
- **Open PRs as drafts** (`gh pr create --draft`) and say in the description what
  you verified and what you did not.
- **Report honestly.** If tests fail, say so and paste the output. If part of the
  task was skipped, say which part and why. A partial result described accurately
  is more useful than a complete-sounding one that isn't.
- **State assumptions rather than blocking.** If a detail is ambiguous, pick the
  reading a careful colleague would, write it down in the PR, and keep going.

## Commits

Conventional Commits, imperative mood, scoped where it helps:

```
feat(analysis): derive tempo from beat grid rather than storing BPM
fix(audio): remove allocation from the capture callback
docs: record why the pre-roll buffer is 8 seconds
```
