# AGENTS.md

Instructions for any agent working in this repository. Humans should read it
too — it is the short version of how this project is built.

## What this is

`motif` is a terminal groovebox: it listens to you play, captures a loop aligned
to bar one, infers the musical structure of what you played, and uses that
structure to help build a song sketch.

**Read `docs/README.md` before your first change.** It is the founding brief:
short, and it explains *why* the architecture is shaped the way it is. Most bad
changes to this codebase come from not knowing the reasoning in it. Read it as
history and intent, not as a description of what exists — see 1.7.

Status: early. The crate is a skeleton. v1 scope is passthrough, loop capture,
beat grid, tempo, downbeat, and one generated MIDI part.

## Commands

```sh
cargo build
cargo test
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-style.sh
cargo check --target aarch64-unknown-linux-gnu
```

Run all of them before opening a pull request. A red CI on a draft PR is noise
for everyone.

## Design invariants

These four are load-bearing. A change that breaks one is wrong even if it
compiles, passes tests, and looks tidier.

1. **Analysis is retrospective, not causal.** The loop is captured first and
   analysed afterwards, because offline downbeat tracking is ~25–30 points more
   accurate than online. There is a *deadline*, not a latency budget. Do not
   introduce a causal or streaming analyser to "reduce latency" — that trades
   away the project's central advantage.

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

Corollary: **everything cross-compiles to aarch64.** Check before adding a
dependency.

---

## 1. Documentation

Documentation is a first-class citizen of this project, not a step at the end.

**1.1 Clear and concise.** Say the thing and stop. A paragraph that restates the
function signature in English is worse than nothing, because it has to be
maintained and it will go stale.

**1.2 Prefer in-code documentation over documents.** A doc comment sits next to
the thing it describes, is reviewed in the same diff, and cannot drift out of
sync unnoticed. A standalone file cannot. If it explains *how something works*,
it is a doc comment — no exceptions. Prose outside the code is reserved for the
things that have no home in it at all, and 1.6 bounds what that can be.

**1.3 Only public members have doc comments, and every public member has one.**
Public means it appears in `cargo doc`. `missing_docs` is warned and CI runs with
`-D warnings`, so an undocumented public item fails the build. Private items are
not part of any contract; a doc comment on one is clutter that outlives the code
it describes.

**1.4 Inline comments are not allowed.** The code explains itself. If you feel
the need to write `//`, that is a signal — name the value, or extract the step
into a function whose name is the sentence you were going to write.

> There is one case this rule collides with: a threshold from a paper, a constant
> measured against real audio. That provenance is not derivable from the code and
> must not be lost. Put it in the doc comment of the **public item that uses it**,
> where a reader of `cargo doc` will also see it. Do not put it in a `//`, and do
> not put it on a private constant.

**1.5 Tests are documentation.** A reader should be able to learn what a type
does by reading its tests. Name them as sentences about behaviour —
`tempo_is_derived_from_beat_count`, not `test_tempo_2`. Prefer several small
tests that each state one fact over one large test that exercises everything.

Doc examples are run by `cargo test`, which makes them the only documentation
that cannot silently become wrong. Use them for anything with a non-obvious
calling pattern.

**1.6 The only documentation outside the code is a per-folder `README.md`.** One
per folder, describing that folder. There is no `docs/architecture.md`, no
decision-record directory, no design note that lives alongside the thing it
describes rather than inside it. Documentation trees rot from the leaves inward,
and nobody notices because nothing links to the leaves.

Four root files are exempt, because they are the project's contract rather than
documentation of it, and because GitHub looks for them by name: `README.md`,
`AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`. Templates under `.github/` are
configuration.

**1.7 Never document a feature that does not exist yet.** No "coming soon", no
roadmap section, no doc comment on a stub describing what it will eventually do.
Documentation states what the code does today.

Unbuilt work belongs in a **GitHub issue**, where it is queryable, can block and
be blocked by other work, and gets closed when it lands. Aspirational prose in a
repository does none of that: no test contradicts it, no compiler checks it, and
it silently becomes a lie the moment the plan changes.

A `README.md` may state what the project is *for* — its purpose is not a feature
claim. It may not describe behaviour a user cannot get today.

There is exactly one exemption, and it does not generalise: **`docs/README.md` is
the founding brief.** A brief is an input to the project rather than a
description of it, and it is the only file in this repository where
forward-looking statements are allowed. It is dated, it is not updated to track
what gets built, and nothing should cite it as evidence that a feature exists.
Everything in its scope section is destined to become issues.

## 2. Development

**2. All code is developed test-first.** Write the test, watch it fail for the
right reason, then write the code that makes it pass.

**2.1 Only public members are tested.** Implementation details must be free to
change without touching a single test. This is enforced by construction: **all
tests live in `tests/`**, where they compile as an external consumer of the crate
and the compiler makes private items unreachable. `#[cfg(test)]` modules inside
`src/` are rejected by `scripts/check-style.sh`.

A test that needs a private item is telling you one of two things: the item
should be public, or you are testing the wrong thing.

**2.2 Plan the tests carefully; make them go red, then green.** Red first is not
ceremony. A test that has never failed has never demonstrated that it can.

Nothing in a diff reveals the order things were written in, so this clause is on
your honour — but its *purpose* is checked. CI runs `cargo-mutants`, which alters
the code under test and fails if no test notices. Tests written after the fact,
to mirror an implementation, tend to die on that check.

**Testing analysis code.** Accuracy claims need a measurement. "This improves
downbeat detection" is not reviewable; a number against a fixture set is. Commit
small fixtures under `tests/fixtures` and keep them to a few hundred KB.

## 3. Trunk-based development

**3.1 Branches are short-lived.** Hours or a day, not a week. If a change cannot
be finished that fast, it is really several changes — split it and merge the
first one.

**3.2 Merge small and often.** A large pull request is not reviewable in any
meaningful sense; it gets approved rather than read. Prefer a series of small
merged changes over one branch that accumulates.

**3.3 `main` is always green.** Never commit to `main`, never force-push, never
merge your own pull request. Every check must pass before a merge, and a change
that breaks `main` is reverted first and diagnosed second.

**3.4 Incomplete work sits behind a feature flag.** Use Cargo features, named for
the capability rather than the ticket. CI builds both with default features and
with `--no-default-features`, so a flag that only compiles when enabled is caught.
This is what makes 3.1 and 3.2 possible: unfinished work can be merged safely
because it is unreachable.

`TODO` and `FIXME` markers are rejected by `scripts/check-style.sh`. Incomplete
work goes behind a flag and into a GitHub issue, where it is visible and can
block other work. A marker in a source file is neither.

## 4. Pull requests

**4.1 Be concise and descriptive.** A pull request is read by someone deciding
whether to trust the change. Give them what they need and nothing else.

**4.2 The title is the summary.** It must be clear from the title alone what the
change does — most people will never read further, and the title is what shows up
in `main`'s history after a squash merge. Use the commit format,
`type(scope): summary`.

**4.3 The body says what changed and why, not how.** The code is the account of
how. Do not restate the diff in prose, walk through the implementation, or
explain a function that a reader can simply open. Explain the problem, the
decision, and anything a reviewer could not infer — a trade-off taken, an
alternative rejected, a risk accepted.

**4.4 Titles stay under 50 characters.** This is the width `git log --oneline`
gives you before truncating. If the change will not fit, it is usually two
changes.

**4.5 Never add a co-author.** No `Co-authored-by:` trailer on any commit, no
tool attribution in any pull request body. Agents included, and agents
especially — the tool that produced a change is not a fact about the change, and
the history should not be a record of what was fashionable.

---

## How these are enforced

Rules that live only in this file get broken. Each one is pushed as far up this
ladder as it goes:

| Rule | Enforced by | Tier |
|---|---|---|
| 2.1 only public members tested | the compiler — tests in `tests/` cannot see private items | impossible to break |
| 1.3 every public item documented | `missing_docs` + `-D warnings` | CI fails |
| 1.4 no inline comments | `scripts/check-style.sh` | CI fails |
| 1.6 prose outside code is folder READMEs only | `scripts/check-style.sh` | CI fails |
| 1.7 no documentation of unbuilt features | `scripts/check-style.sh`, on the phrases that give it away | CI fails |
| 3.4 no `TODO` markers | `scripts/check-style.sh` | CI fails |
| 3.4 feature flags compile both ways | default and `--no-default-features` builds | CI fails |
| 1.5 doc examples stay true | `cargo test --doc` | CI fails |
| 2.2 tests actually constrain behaviour | `cargo-mutants` on the diff | CI fails |
| 3.3 `main` green | every check above is a **required status check** on `main` | merge blocked |
| 3.3 no direct pushes to `main` | ruleset — pull request required, force-push and deletion blocked | push rejected |
| 3.2 linear history | ruleset — squash or rebase merges only | merge blocked |
| 4.2 title format | `PR hygiene` workflow | CI fails |
| 4.4 title under 50 characters | `PR hygiene` workflow | CI fails |
| 4.5 no co-authors, no tool attribution | `PR hygiene` workflow, over every commit and the body | CI fails |
| 4.1, 4.3 concision, no implementation walkthrough | review | review |
| 1.3 private items *not* documented | review — no lint exists for the inverse | review |
| 1.1, 1.2, 1.5 clarity | review, plus the pull request checklist | review |
| 2.2 red-before-green ordering | not observable in a diff | honour |
| 3.1, 3.2 branch size and age | review | review |

The `main` branch carries a ruleset with **no bypass actors**, deliberately. An
agent runs with the repository owner's credentials, so an admin exemption would
be an exemption for every agent too, and 3.3 would be back to an honour system.
Turning the ruleset off is a visible, deliberate act rather than an accident.

Because the pull request rule requires branches to be up to date before merging,
a long-lived branch has to keep rebasing onto a moving `main`. That friction is
the point: it makes 3.1 cheaper to obey than to ignore.

One gap remains: **nothing stops a doc comment on a private item.** Clippy has a
lint for the opposite case only, and catching this properly needs an AST walk
rather than a grep. It is on review until someone writes that check.

## Task tracking

**GitHub Issues is the single source of truth** for work done and to be done.
Not a markdown TODO list, not a second tracker. Dual tracking is the main way a
project like this loses track of itself.

Find work that is actually actionable — open, with no open blockers:

```sh
gh issue list --search "is:open -is:blocked"
```

> This must go through `gh issue list --search`, which queries GitHub's
> *advanced* search index. The legacy index (raw GraphQL `search(type: ISSUE)`)
> silently ignores dependency qualifiers and returns wrong results with no error.
> If you are writing a raw query, you want `ISSUE_ADVANCED` in GraphQL or
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
  unavailable here. Use **labels** for metadata.
- Writes are subject to a secondary rate limit of 80/minute and 500/hour across
  the whole account. When creating many issues, space them by at least a second.
- Post progress with `gh issue comment N --edit-last --create-if-none --body ...`
  so a long task leaves one updating comment rather than a wall of them.

## Working agreement

- **One task per session, one pull request per task.** Scope creep in a pull
  request is the most expensive thing an agent can do here.
- **Branch from `main`**, and open pull requests as drafts (`gh pr create
  --draft`).
- **New dependencies need a reason in the pull request description**: what it
  does, why not hand-rolled, its licence, and that it cross-compiles.
- **No `unwrap()` or `expect()` on paths that can fail at runtime.** Setup-time
  invariants and tests are fine; the audio path and user input are not.
- **Match the surrounding code** — its naming and idiom win over personal
  preference.
- **Report honestly.** If tests fail, say so and paste the output. If part of the
  task was skipped, say which part and why. A partial result described accurately
  is more useful than a complete-sounding one that isn't.
- **State assumptions rather than blocking.** If a detail is ambiguous, pick the
  reading a careful colleague would, write it down in the pull request, and keep
  going.

## Commits

Conventional Commits, imperative mood, scoped where it helps, subject under 50
characters:

```
feat(analysis): derive tempo from the beat grid
fix(audio): drop allocation from capture callback
test(analysis): pin downbeat against 3/4 fixture
```

No trailers. No `Co-authored-by:`, no tool attribution, no session links — see
4.5. The body is optional; use it for a decision a future reader would otherwise
have to reconstruct, not for a description of the diff.
