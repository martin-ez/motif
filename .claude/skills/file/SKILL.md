---
name: file
description: Track new scope that has no issue yet — a bug found mid-task, a follow-up, a dependency that needs replacing. Checks for duplicates, sets area, kind and size, and wires the dependency edges so it does not sink to the bottom of the queue. Use when something needs an issue, when tempted to write TODO or FIXME, or when work turns up that is out of scope for the task in hand.
---

# File new scope

**File it, wire it, keep going.** This usually fires in the middle of another
task, and the whole point is that you return to that task afterwards. Scope creep
is the most expensive thing that happens here.

To decide whether it is even a separate issue: if the fix is needed to make the
claimed issue's tests pass, it is part of the current task. If it can be
described without reference to the current task, it is a new issue.

This is also what to do instead of a marker comment. `TODO`, `FIXME`, `XXX` and
`HACK` are rejected by `check-style.sh` — incomplete work goes behind a Cargo
feature named for the capability, and into an issue where it is queryable and can
block other work.

## 1. Check it is not already filed

```sh
scripts/track.sh find ring
```

This matches titles across **open and closed** issues, locally, without touching
the search index. Closed matches are the ones that matter: something filed,
rejected and closed last week is exactly what gets filed again.

It matches titles only, so a duplicate worded differently still slips through.
Try the two or three words someone else would have reached for, not just your
own. If you suspect a duplicate you cannot find, say so in the body and let a
human close it.

## 2. Set the three required labels

`add` refuses without all three.

- **area** — `engine`, `seq`, `synth`, `ui`, `io`, `infra`
- **kind** — `feat`, `bug`, `chore`, `spike`
- **size** — `s` well under one session, `m` about one session, `l` too big to
  claim

`size:l` cannot be claimed at all. If the answer is honestly `l`, either split it
immediately with `--parent`, or file it as an epic knowing it is a container and
its children are the real work.

## 3. Write a body someone can pick up cold

Context, then a `### Done when` that names an observable outcome. "The meter
reads within 1 dB of the input" is testable; "metering works" is not.

- Touching the audio callback, the beat grid, or the UI backend boundary? Name
  the invariant it must not break.
- Making an accuracy claim? It needs a fixture set and a metric, which usually
  means a dependency on the fixture harness rather than a sentence.

An issue is the right home for unbuilt work — that is what rule 1.7 pushes it
here for. Be concrete about the problem and the done condition; leave the
implementation to whoever claims it.

## 4. Wire the edges

**The step that gets skipped.** `ready` sorts by how much each issue unblocks, so
an issue filed with no edges sorts last and stays invisible indefinitely.

Ask what someone would reach for on day one of this work. If that is an open
issue, it is a blocker.

```sh
scripts/track.sh add -t 'Report the actual block size after negotiation' \
  --area io --kind bug --size s -F body.md \
  --blocked-by 75 --blocking 88
```

`--parent N` instead, if it belongs to an epic rather than depending on one.

## 5. Confirm it landed where you meant

```sh
scripts/track.sh ready
```

If it should be ready and is not, an edge is wrong. If it appeared with no
`(unblocks …)` count, reconsider whether it really holds nothing up — that is
usually a missing `--blocking` rather than a genuinely isolated task.

## 6. Go back to what you were doing

Reference the new number from the current pull request body if it is related.
Do not start on it: one task per session.
