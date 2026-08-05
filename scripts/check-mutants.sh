#!/usr/bin/env bash
# scripts/check-mutants.sh — CI's mutation sweep, run before the pull request.
#
#   scripts/check-mutants.sh          sweep this branch's changes to main
#   scripts/check-mutants.sh <ref>    sweep against another base
#
# cargo-mutants alters the changed code and fails when no test notices. It is
# the one check that reads a diff for whether its tests constrain behaviour
# rather than merely pass, and so the only one that can fail a change every
# other command in the gate approved.
#
# Scoped to the diff, as in CI, so the runtime stays proportional to the change.
# The diff runs from the merge base to the working tree rather than to HEAD:
# CI answers for what was pushed, and a local run should answer for what is
# about to be. Untracked files are in neither, so they are named instead.
#
# Exit codes:
#   0  every mutant in the diff was caught, or it changes no testable code
#   1  a mutant survived, or the sweep could not run
#
# Written for bash 3.2 (macOS /bin/bash).

set -euo pipefail

cd "$(dirname "$0")/.."

die()  { printf '\n\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
note() { printf '%s\n' "$*" >&2; }
pass() { printf '\033[32mok\033[0m    %s\n' "$*"; }

case "${1:-}" in
-h | --help)
	sed -n '2,6p' "$0" | cut -c 3-
	exit 0
	;;
esac

base="${1:-}"
if [ -z "$base" ]; then
	if git rev-parse --verify --quiet origin/main >/dev/null; then
		base=origin/main
	else
		base=main
	fi
fi
git rev-parse --verify --quiet "$base" >/dev/null \
	|| die "no such ref: $base"

merge_base="$(git merge-base "$base" HEAD)" \
	|| die "$base and HEAD share no history — nothing to diff against."

# A missing subcommand otherwise surfaces as cargo's "no such command", which
# reads as a broken repository rather than a tool this checkout has not
# installed. CI installs it per run; a working copy installs it once.
cargo mutants --version >/dev/null 2>&1 || die "cargo-mutants is not installed.

      cargo install --locked cargo-mutants"

untracked="$(git ls-files --others --exclude-standard -- '*.rs')"
if [ -n "$untracked" ]; then
	note "These files are untracked, so no diff contains them and the sweep
cannot see them. Stage them with \`git add\` to have them swept:"
	printf '%s\n' "$untracked" | sed 's/^/  /' >&2
	note ""
fi

diff="$(mktemp "${TMPDIR:-/tmp}/motif-mutants.XXXXXX")"
trap 'rm -f "$diff"' EXIT
git diff "$merge_base" >"$diff"

if [ ! -s "$diff" ]; then
	pass "no changes against $base — nothing to sweep"
	exit 0
fi

note "sweeping the diff with $base ($(git rev-parse --short "$merge_base")) …"

# One job per core, as in CI, where the sweep sets the wall clock for the whole
# pull request. cargo-mutants warns above eight: each job is a cargo process
# that starts threads of its own, so past that the machine thrashes rather than
# finishes sooner. --no-shuffle keeps two runs of the same diff comparable.
jobs="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"
[ "$jobs" -le 8 ] || jobs=8
cargo mutants --in-diff "$diff" --no-shuffle -j "$jobs" \
	|| die "a mutant survived. The report is in mutants.out/.

      Each one is a change to the code that no test objected to. Write the
      test that would have caught it (AGENTS.md 2, 2.2), rather than
      reaching for a skip."

pass "every mutant in the diff was caught"
