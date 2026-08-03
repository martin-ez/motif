---
name: file
description: Track new scope that has no issue yet — a bug found mid-task, a follow-up, a dependency to replace. Use when something needs an issue, when tempted to write TODO or FIXME, or when work turns up outside the task in hand.
---

# File new scope

**File it, wire it, keep going.** This fires in the middle of another task, and
the point is that you return to that task afterwards.

Is it separate work? If the fix is needed to make the claimed issue's tests pass,
it belongs to the current task. If it can be described without reference to that
task, it is a new issue. This is also what to do instead of a `TODO` marker.

## 1. Check it is not already filed

```sh
scripts/track.sh find ring
```

Matches titles across open **and closed** issues. The closed ones are what
matter: something filed, rejected and closed last week is exactly what gets filed
again. Titles only, so try the words someone else would have reached for, not
just your own.

## 2. Label it

`--area`, `--kind` and `--size`, all three required. `size:l` cannot be claimed —
either split it immediately with `--parent`, or file it knowing it is a container
whose children are the real work.

## 3. Write a body that reads cold

Context, then a `### Done when` naming an observable outcome. Name the invariant
if it touches the audio callback, the beat grid or the UI backend. An accuracy
claim needs a dependency on the fixture harness, not a sentence.

## 4. Wire the edges

**The step that gets skipped.** `ready` sorts by how much each issue unblocks, so
one filed with no edges sorts last and stays invisible.

Ask what someone would reach for on day one of this work. If that is an open
issue, it is a blocker.

```sh
scripts/track.sh add -t 'Report the block size after negotiation' \
  --area io --kind bug --size s -F body.md --blocked-by 75 --blocking 88
```

`--parent N` instead, when it belongs to an epic rather than depends on one.

## 5. Confirm, then go back

`scripts/track.sh ready` — if it should be ready and is not, an edge is wrong. If
it landed with no `(unblocks …)` count, that is usually a missing `--blocking`.

Reference the new number from the current pull request if it is related. Do not
start on it.
