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

The list is sorted by how much each row unblocks, so the top row frees the most
work. Take the top three rows and put them to the user with `AskUserQuestion` —
one option per issue, labelled `#74`, described by its title, size and what it
unblocks, top row first and marked `(Recommended)`. Their pick is the one to
claim; claim nothing before they answer.

Offer fewer than three if `ready` lists fewer. Anything under `SPLIT:` is
`size:l` and unclaimable — leave it out of the options and say so, or use the
`refine` skill. If the user named an issue, still run `ready`: take it without
asking if it is there, and say why rather than forcing it if it is not.

## 3. Take

```sh
scripts/track.sh start 74
```

The issue the user picked. Claims first, then branches onto `feat/74-…`. Exit 2
means someone else took it in the meantime: say so and ask again with the rows
that are left, never `--force`.

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

Link the issue in the body on its own line — `Tracks #74`, never `Closes`.

**Do not close the issue and do not merge.** Merging settles it: the `tracking`
workflow runs `done` for every issue the body tracks, and `release` if the pull
request is closed unmerged. Closing it by hand earlier has the tracker assert
work is in `main` when it is not.

Abandoning without a pull request: `scripts/track.sh release 74`.
