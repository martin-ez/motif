---
name: next
description: Pick up the next piece of work. Lists unblocked, unclaimed issues from the tracker, claims one, branches, and starts the test-first loop. Use at the start of a session, or when asked what to work on next, what is ready, or to take the next task.
---

# Take the next task

One task per session, one pull request per task. This skill ends with a draft
pull request open and the issue still claimed — not with a merge.

## 1. Check the tree is clean

```sh
git status --short && git switch main && git pull --ff-only
scripts/track.sh mine
scripts/track.sh doctor
```

A dirty tree means an unfinished task. Finish or release that one first.

`mine` answers "was I already working on something?" — the question to ask after
a crash, a restart, or a compacted context, when the branch name is the only
other clue. If it lists an issue, finish or release that before taking new work.

## 2. Read the queue

```sh
scripts/track.sh ready
```

Take the **top row**. It sorts by how much each issue unblocks, so the top row
frees the most downstream work.

Exit codes are load-bearing: **any non-zero exit other than 2 is fatal** — surface
stderr verbatim and stop. Never fall back to `gh issue list` or `gh search`. The
legacy search index ignores `is:blocked` and returns blocked issues as ready with
a 200 and no error, which is the exact failure `track.sh` exists to make
unreachable.

If `ready` lists something under `SPLIT:`, it is `size:l` and cannot be claimed.
Split it with the `refine` skill, or take the next row.

If the user named an issue, use theirs — but run `ready` anyway and confirm it
appears. If it does not, say why (blocked, claimed, or a container) rather than
forcing it.

## 3. Take it

```sh
scripts/track.sh start 74
```

`start` claims the issue and then puts you on a branch named for it —
`feat/74-…`, derived from the issue's kind and title. That order matters and is
why the command exists: the claim is the part that can fail, so a contended issue
never leaves a stray branch behind.

- **exit 0** — it is yours, and you are on the branch.
- **exit 2** — `busy #74 holder=…`. Someone else has it. Do not wait, do not
  `--force`. Go back to the ready list and take the next row.
- **exit 1** — fatal. Surface stderr and stop.

It refuses a dirty working tree, and `claim` beneath it refuses a `size:l` issue
or a container with open sub-issues. Both of those mean the same thing: take a
child instead.

The branch is cut from `main`, which is why step 1 pulls first.

## 5. Read the whole issue before writing any code

```sh
scripts/track.sh show 74
```

Read the body, `needs`, and `blocks`. Then restate the scope in one or two
sentences and check it against the design invariants in AGENTS.md — especially
whether the change touches the audio callback, the beat grid, or the UI backend
boundary.

If the issue turns out to be several tasks, stop and split it with
`scripts/track.sh add --parent 74 …` rather than doing all of it. Scope creep is
the most expensive thing that can happen here.

## 6. Test first

Write the test in `tests/`, run it, and **watch it fail for the right reason**
before writing any implementation. A test that has never failed has never
demonstrated it can.

Tests reach only the public API — that is enforced by their location, not by
convention. Name them as sentences: `tempo_is_derived_from_beat_count`.

If the issue makes an accuracy claim, it needs a number against fixtures in
`tests/fixtures`, not a description.

## 7. Run the full gate

```sh
cargo build
cargo test --all-features
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
scripts/check-style.sh
cargo check --target aarch64-unknown-linux-gnu
```

All six, every time. The cross-check catches a dependency that does not build for
the target device, which is cheapest to find now.

## 8. Open a draft pull request

Title is the summary and lands in `main`'s history: `type(scope): summary`, at
most 50 characters. Body is context then `### Changes` — no verification section,
no walkthrough of the diff, **no co-author or tool-attribution trailer**.

State any assumption you made in the body rather than asking and blocking.

## 9. Stop there

Do not close the issue and do not merge. `scripts/track.sh done 74 -m "…"` runs
**after** someone else merges the pull request — closing it earlier makes the
tracker claim work exists that is not in `main`.

If you are abandoning the task, release it so someone else can take it:

```sh
scripts/track.sh release 74
```
