---
name: next
description: Take the next piece of work off the tracker — pick a ready issue, claim it, branch, and run the test-first loop to a draft pull request. Use at the start of a session, or when asked what to work on next or what is ready.
---

# Take the next task

The sequence only. AGENTS.md is already in context; its rules on test-first work,
pull requests and exit codes apply here without being restated.

## 1. Orient

```sh
git switch main && git pull --ff-only
scripts/track.sh mine
scripts/track.sh doctor
```

`mine` answers "was I already working on something?" — the question after a
crash, a restart, or a compacted context. If it lists an issue, finish or release
that before taking new work. A dirty tree means the same thing.

## 2. Pick

```sh
scripts/track.sh ready
```

Top row. Anything under `SPLIT:` is `size:l` and unclaimable — use the `refine`
skill, or take the next row. If the user named an issue, still run `ready`, and
say why if it is absent rather than forcing it.

## 3. Take

```sh
scripts/track.sh start 74
```

Claims first, then branches onto `feat/74-…`. Exit 2 means someone else has it:
take the next row, never `--force`.

## 4. Read it whole

```sh
scripts/track.sh show 74
```

Restate the scope in a sentence, and name any design invariant it touches. If it
is really several tasks, split it with `add --parent` instead of doing all of it.

## 5. Build

Test first, in `tests/`, and watch it fail for the right reason. Then the full
gate from AGENTS.md — all six, including the aarch64 cross-check.

## 6. Stop at a draft pull request

**Do not close the issue and do not merge.** `scripts/track.sh done 74 -m "…"`
runs only after someone else merges; closing it earlier has the tracker assert
work is in `main` when it is not.

Abandoning instead: `scripts/track.sh release 74`.
