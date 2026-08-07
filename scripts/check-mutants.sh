#!/usr/bin/env bash
# scripts/check-mutants.sh — CI's mutation sweep, run before the pull request.
#
#   scripts/check-mutants.sh             sweep this branch's changes to main
#   scripts/check-mutants.sh <ref>       sweep against another base
#   scripts/check-mutants.sh --selftest  check the rules the sweep runs on
#
# MOTIF_MUTANTS_JOBS overrides the job count picked for this machine.
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
#   1  a mutant survived, the sweep timed out, or it could not run
#
# Written for bash 3.2 (macOS /bin/bash).

set -euo pipefail

cd "$(dirname "$0")/.."

die()  { printf '\n\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
note() { printf '%s\n' "$*" >&2; }
pass() { printf '\033[32mok\033[0m    %s\n' "$*"; }

cores() { getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4; }

# A quarter of the cores, at least one, and never more than eight.
#
# A job is not a core's worth of work. cargo-mutants measures a baseline on an
# unmutated tree, derives every mutant's timeout from it, then runs that many
# whole test suites at once — and several of this crate's tests spin threads of
# their own. Give the machine no headroom and each mutant's wall clock stretches
# past a timeout measured when nothing else was running.
#
# Measured on a 12-core laptop against a clean diff: eight jobs returned 38
# caught and 12 timeouts, three returned 50 caught and none. Eight is where
# cargo-mutants warns, and a quarter of the cores only reaches it at 32.
jobs_for() {
	local jobs=$(($1 / 4))
	[ "$jobs" -ge 1 ] || jobs=1
	[ "$jobs" -le 8 ] || jobs=8
	printf '%s\n' "$jobs"
}

sweep_jobs() { printf '%s\n' "${MOTIF_MUTANTS_JOBS:-$(jobs_for "$(cores)")}"; }

# cargo-mutants separates its failures by exit code and the sweep must too: it
# ranks a timeout above a survivor, so a run that timed out exits 3 whether or
# not anything also survived, and says nothing either way about whether the
# diff's tests constrain behaviour. Reporting that as a survivor sends an agent
# to write a test for a mutant nothing is wrong with.
explain() {
	case "$1" in
	0) ;;
	2)
		printf '%s' "a mutant survived. The report is in mutants.out/.

      Each one is a change to the code that no test objected to. Write the
      test that would have caught it (AGENTS.md 2, 2.2), rather than
      reaching for a skip."
		;;
	3)
		printf '%s' "the sweep timed out, so it decided nothing.

      A timeout is not a survivor. cargo-mutants derives each mutant's
      timeout from a baseline measured on an unmutated tree, so under load a
      mutant stalls in whichever test binary happened to be running rather
      than in the code it touched.

      Jobs for this run: $2. Re-run with fewer before writing any test:

        MOTIF_MUTANTS_JOBS=$(($2 > 1 ? $2 / 2 : 1)) scripts/check-mutants.sh

      At one job a timeout is a real hang: find the mutant in mutants.out/
      and the loop it caused."
		;;
	4)
		printf '%s' "the tests already fail without a mutant, so nothing was swept.

      Fix \`cargo test\` first. The sweep asks a question about the tests and
      cannot ask it of a tree that is already red."
		;;
	*)
		printf '%s' "the sweep could not run (cargo mutants exited $1).

      This is no verdict on the diff. 1 is a usage error, 5 and 6 mean the
      diff did not match the tree or did not parse; anything else is a
      cargo-mutants fault. Its output above says which."
		;;
	esac
}

# The sweep has no Rust to be tested from tests/, so it carries its cases with
# it the way the pull-request body check does, and they run wherever the script
# can change.
st_is() {
	local want="$1" got="$2" name="$3"
	if [ "$got" = "$want" ]; then
		printf '\033[32mok\033[0m    %s\n' "$name"
	else
		printf '\033[31mFAIL\033[0m  %s (got "%s", wanted "%s")\n' \
			"$name" "$got" "$want"
		st_status=1
	fi
}

st_says() {
	local status="$1" want="$2" name="$3"
	case "$(explain "$status" 4)" in
	*"$want"*) printf '\033[32mok\033[0m    %s\n' "$name" ;;
	*)
		printf '\033[31mFAIL\033[0m  %s (never says "%s")\n' "$name" "$want"
		st_status=1
		;;
	esac
}

st_omits() {
	local status="$1" unwanted="$2" name="$3"
	case "$(explain "$status" 4)" in
	*"$unwanted"*)
		printf '\033[31mFAIL\033[0m  %s (says "%s")\n' "$name" "$unwanted"
		st_status=1
		;;
	*) printf '\033[32mok\033[0m    %s\n' "$name" ;;
	esac
}

selftest() {
	st_status=0

	st_is 3 "$(jobs_for 12)" "twelve cores take three jobs"
	st_is 2 "$(jobs_for 8)" "eight cores take two"
	st_is 1 "$(jobs_for 4)" "four cores take one"
	st_is 1 "$(jobs_for 1)" "a single core takes one"
	st_is 8 "$(jobs_for 32)" "thirty-two cores reach the cap"
	st_is 8 "$(jobs_for 64)" "the cap holds above it"

	st_is 5 "$(MOTIF_MUTANTS_JOBS=5 sweep_jobs)" "an override beats the rule"
	st_is "$(jobs_for "$(cores)")" "$(sweep_jobs)" "no override leaves the rule"

	st_says 2 "a mutant survived" "a survivor is named as one"
	st_says 3 "timed out" "a timeout is named as one"
	st_omits 3 "a mutant survived" "a timeout is not called a survivor"
	st_says 3 "MOTIF_MUTANTS_JOBS" "a timeout names the lever to pull"
	st_says 4 "already fail" "a red tree is named as one"
	st_says 6 "could not run" "an unrecognised status decides nothing"
	st_is "" "$(explain 0 4)" "a clean sweep explains nothing"

	if [ "$st_status" = 0 ]; then
		printf '\033[32mok\033[0m    every rule the sweep runs on holds\n'
	fi
	return "$st_status"
}

case "${1:-}" in
-h | --help)
	sed -n '2,8p' "$0" | cut -c 3-
	exit 0
	;;
--selftest)
	selftest || exit 1
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

# --no-shuffle keeps two runs of the same diff comparable.
#
# --cap-lints=true because the sweep runs under -D warnings, which CI sets at
# workflow level and scripts/gate.sh mirrors. Without it a mutant that trips a
# lint is reported unviable rather than tested, and an unviable mutant is not
# counted: measured on src/ui/hold.rs, 10 of 11 mutants caught with the flag
# and 5 of 11 without. The same flag is on ci.yml's invocation, and
# scripts/gate.sh --selftest fails if the two stop agreeing.
jobs="$(sweep_jobs)"
status=0
cargo mutants --in-diff "$diff" --no-shuffle --cap-lints=true -j "$jobs" || status=$?
[ "$status" = 0 ] || die "$(explain "$status" "$jobs")"

pass "every mutant in the diff was caught"
