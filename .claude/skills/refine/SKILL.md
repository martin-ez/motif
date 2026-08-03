---
name: refine
description: Sharpen the issues at the front of the dependency graph before anyone claims them — split oversized ones, add missing dependencies, and turn thin descriptions into testable acceptance criteria, through conversation with the user. Use when grooming the backlog, splitting a size:l epic, or when ready work looks underspecified.
---

# Refine the frontier

Work the *front* of the graph only — issues that are ready now, or one hop
behind. Refining a deep node is guessing: its shape depends on decisions that
have not been made yet, and the guess will be rewritten.

This skill is conversational. Propose, ask, then apply. Nothing here is applied
silently.

## 1. Pull the graph

```sh
scripts/track.sh graph --json > /tmp/graph.json
scripts/track.sh ready
```

The frontier is every issue with `blockers == []`, plus anything `ready` prints
under `SPLIT:`.

Also worth surfacing from the same payload:

- **`CYCLE:` in the `ready` output** — issues that block each other and are
  unreachable from any root. Always fix these first; they strand real work.
- **`size:l` on the frontier** — unclaimable until split. Usually the reason the
  ready queue looks emptier than it should.
- **A leaf with no `parent` and empty `unblocks`** — often a missing edge rather
  than genuinely independent work.

## 2. Assess each candidate

`scripts/track.sh show <n>`, then check it against these. Report what fails,
not a general impression.

- **One session?** `size:s` or `size:m`. If the body describes two verbs on two
  nouns, it is two issues.
- **Testable done condition?** "Done when" has to name an observable outcome. "A
  meter renders" is testable; "metering works" is not.
- **Invariant named?** If it touches the audio callback, the beat grid, or the UI
  backend boundary, the issue should say which invariant constrains it — that is
  what stops a reviewer approving a change that compiles and is still wrong.
- **Accuracy claim without a measurement?** Any issue claiming better detection
  needs a fixture set and a metric, or it is unreviewable. Make that a dependency
  on the fixture harness rather than a sentence in the body.
- **Dependencies complete?** Ask what this work would reach for on day one. If
  that thing is an open issue and not a blocker, the edge is missing.

## 3. Discuss before changing

Bring the user one issue at a time with a concrete proposal — the split you would
make, the edge you would add, the wording you would use. Use AskUserQuestion when
two readings would produce materially different work; decide it yourself when a
careful colleague would.

Do not batch a dozen changes and apply them at the end. The point of this skill
is the conversation.

## 4. Apply, additively

Only through `scripts/track.sh`. Never `gh` directly.

```sh
scripts/track.sh add -t '<part>' --parent 65 --area infra --kind chore --size s -F body.md
scripts/track.sh dep 88 --needs 85
scripts/track.sh dep 88 --drop-needs 79
scripts/track.sh note 65 -m 'Split into #96-#98; scoring tolerance settled at ±70 ms.'
```

**Refinement is additive.** There is no body-edit command, and that is the right
constraint: a change of scope becomes a comment and new sub-issues, so the
issue's history stays readable rather than being quietly rewritten under a
reviewer who already read it.

When a parent gains children, it stops being claimable and starts being a
container — which is correct, and means the children carry the `area`/`kind`/
`size` that matter.

## 5. Confirm the graph moved

```sh
scripts/track.sh ready
```

New children should appear as ready, the `SPLIT:` line should be gone, and any
cycle should be resolved. If the queue did not change, the refinement did not
land — say so rather than reporting success.

## Scope

Stop when the frontier is clean. Leaving a distant epic as a one-paragraph stub
is the correct state for it, not an omission — it gets refined when the work in
front of it lands and its shape is known.
