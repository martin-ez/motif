#!/usr/bin/env bash
# scripts/gate.sh — every check CI can fail a pull request on, run before it does.
#
#   scripts/gate.sh              the whole gate, sweep included
#   scripts/gate.sh --no-sweep   everything but the mutation sweep
#   scripts/gate.sh --selftest   check the rules the gate runs on
#
# RUSTFLAGS and RUSTDOCFLAGS carry -D warnings for the whole run, because
# .github/workflows/ci.yml sets them at workflow level and so every job there
# inherits them. A warning that only denies in CI is the drift this exists to
# close, and it costs a rebuild the first time the gate runs over ad-hoc work.
#
# Two of CI's checks are absent because they are not knowable from a working
# tree: the pull request title and body are properties of a request that does
# not exist yet. Draft the body and run `scripts/check-pr-body.sh -F body.md`.
#
# Every check runs even after an earlier one fails, as `!cancelled()` makes
# them in CI, so one run says everything rather than the first thing. The
# mutation sweep is last and is the exception: it decides nothing on a tree
# whose tests already fail, so a red tree skips it rather than reporting it.
#
# The heartbeat naming each command goes to stderr and the verdicts to stdout,
# so `scripts/gate.sh 2>/dev/null` is the report on its own.
#
# Exit codes:
#   0  every check that ran passed
#   1  a check failed, or the gate could not run one
#
# Written for bash 3.2 (macOS /bin/bash).

set -euo pipefail

cd "$(dirname "$0")/.."

export RUSTFLAGS="-D warnings"
export RUSTDOCFLAGS="-D warnings"

TAB="$(printf '\t')"

die()  { printf '\n\033[31mFAIL\033[0m  %s\n' "$*" >&2; exit 1; }
note() { printf '%s\n' "$*" >&2; }
pass() { printf '\033[32mok\033[0m    %s\n' "$*"; }

# A failure record is a name and the command that reproduces it, so the summary
# can hand back a single line to run rather than a check to go and find. They
# differ wherever the fix is not the check: `--check` reports the formatting,
# `cargo fmt --all` repairs it.
failures=""
last_rc=0

record() {
	local entry="$1$TAB$2"
	if [ -z "$failures" ]; then
		failures="$entry"
	else
		failures="$failures
$entry"
	fi
}

summarise() {
	local failures="$1" sweep="$2" count=0 word=checks name cmd

	if [ -n "$failures" ]; then
		count="$(printf '%s\n' "$failures" | wc -l | tr -d ' ')"
		if [ "$count" = 1 ]; then word=check; fi
		printf '\n\033[31mFAIL\033[0m  %s %s failed:\n\n' "$count" "$word"
		while IFS="$TAB" read -r name cmd; do
			[ -n "$name" ] || continue
			printf '      %s\n        %s\n' "$name" "$cmd"
		done <<EOF
$failures
EOF
		printf '\n'
	fi

	case "$sweep" in
	red)
		printf '      %s\n' \
			"The mutation sweep did not run. It asks whether a diff's tests" \
			"constrain behaviour, and cannot ask that of a tree whose tests" \
			"already fail without a mutant. Fix those, then run the gate again."
		;;
	off)
		printf '\n\033[31m%s\033[0m\n' \
			"      THE MUTATION SWEEP DID NOT RUN — you passed --no-sweep."
		printf '      %s\n' \
			"This is not the gate. The sweep is the only check that can fail a" \
			"change every other one approved, and CI runs it on every pull" \
			"request whether or not you did."
		;;
	esac

	if [ -n "$failures" ]; then
		return 1
	fi
	if [ "$sweep" = ran ]; then
		pass "every check CI can fail a pull request on passed"
	else
		pass "every check that ran passed"
	fi
	return 0
}

# Named before anything runs rather than eight minutes into the gate, and with
# the line that installs the missing piece.
preconditions() {
	rustup target list --installed 2>/dev/null |
		grep -qx aarch64-unknown-linux-gnu ||
		die "the aarch64 target is not installed, so the cross-compile guard
      cannot run.

        rustup target add aarch64-unknown-linux-gnu"

	[ "$1" = no ] || cargo mutants --version >/dev/null 2>&1 ||
		die "cargo-mutants is not installed, so the sweep cannot run.

        cargo install --locked cargo-mutants"
}

run_check() {
	local name="$1" fix="$2" out rc=0
	shift 2
	note "→ $*"
	out="$("$@" 2>&1)" || rc=$?
	last_rc="$rc"
	if [ "$rc" = 0 ]; then
		pass "$name"
		return 0
	fi
	printf '\n\033[31mFAIL\033[0m  %s\n\n' "$name"
	printf '%s\n\n' "$out"
	[ -n "$fix" ] || fix="$*"
	record "$name" "$fix"
	return 0
}

# The commit half of CI's hygiene job. The body half reads a pull request that
# does not exist while the gate runs; this reads the commits, which do.
no_coauthors() {
	local base=main merge_base found
	if git rev-parse --verify --quiet origin/main >/dev/null; then
		base=origin/main
	fi
	merge_base="$(git merge-base "$base" HEAD)" || {
		printf '%s and HEAD share no history, so no range of commits could be read.\n' "$base"
		return 1
	}
	found="$(git log --format='%B' "$merge_base..HEAD" |
		grep -inE '^co-authored-by:' || true)"
	[ -z "$found" ] || {
		printf '%s\n\n%s\n' "$found" \
			"AGENTS.md 4.5: the tool that produced a change is not a fact about
the change. Reword the commit, or drop the trailer with a rebase."
		return 1
	}
}

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

st_has() {
	local text="$1" want="$2" name="$3"
	case "$text" in
	*"$want"*) printf '\033[32mok\033[0m    %s\n' "$name" ;;
	*)
		printf '\033[31mFAIL\033[0m  %s (never says "%s")\n' "$name" "$want"
		st_status=1
		;;
	esac
}

st_hasnt() {
	local text="$1" unwanted="$2" name="$3"
	case "$text" in
	*"$unwanted"*)
		printf '\033[31mFAIL\033[0m  %s (says "%s")\n' "$name" "$unwanted"
		st_status=1
		;;
	*) printf '\033[32mok\033[0m    %s\n' "$name" ;;
	esac
}

# The gate has no Rust to be tested from tests/, so it carries its cases with
# it the way the sweep and the body check do, and they run wherever it can
# change.
selftest() {
	st_status=0

	local out rc two

	rc=0
	out="$(summarise "" ran)" || rc=$?
	st_is 0 "$rc" "a clean gate exits 0"
	st_has "$out" "passed" "a clean gate says so"

	two="clippy (default features)${TAB}cargo clippy --all-targets
cross-compile guard${TAB}cargo check --target aarch64-unknown-linux-gnu --all-features"
	rc=0
	out="$(summarise "$two" ran)" || rc=$?
	st_is 1 "$rc" "a failure exits 1"
	st_has "$out" "2 checks failed" "the summary counts them"
	st_has "$out" "clippy (default features)" "the summary names the first failure"
	st_has "$out" "cross-compile guard" "the summary names the second"
	st_has "$out" "cargo clippy --all-targets" "each failure carries its command"
	st_has "$out" "cargo check --target aarch64-unknown-linux-gnu --all-features" \
		"the command is the one that reproduces it"
	st_hasnt "$out" "passed" "a gate with a failure never says passed"

	rc=0
	out="$(summarise "formatting${TAB}cargo fmt --all" ran)" || rc=$?
	st_has "$out" "1 check failed" "one failure is not called two"
	st_has "$out" "cargo fmt --all" "the fix is offered where it is not the check"

	rc=0
	out="$(summarise "tests${TAB}cargo test --all-features" red)" || rc=$?
	st_is 1 "$rc" "a red tree exits 1"
	st_has "$out" "sweep did not run" "a red tree says the sweep did not run"
	st_has "$out" "already fail" "a red tree says why the sweep was skipped"

	rc=0
	out="$(summarise "" off)" || rc=$?
	st_is 0 "$rc" "--no-sweep with nothing failing still exits 0"
	st_has "$out" "SWEEP DID NOT RUN" "--no-sweep shouts"
	st_has "$out" "--no-sweep" "--no-sweep names the flag that caused it"
	st_has "$out" "every check that ran passed" "--no-sweep still reports what ran"
	st_hasnt "$out" "every check CI can fail" "--no-sweep is not the gate CI runs"

	rc=0
	out="$(summarise "tests${TAB}cargo test --all-features" off)" || rc=$?
	st_is 1 "$rc" "--no-sweep does not rescue a failure"
	st_has "$out" "SWEEP DID NOT RUN" "--no-sweep shouts under a failure too"

	if [ "$st_status" = 0 ]; then
		printf '\033[32mok\033[0m    every rule the gate runs on holds\n'
	fi
	return "$st_status"
}

sweep=yes
while [ $# -gt 0 ]; do
	case "$1" in
	-h | --help)
		sed -n '2,6p' "$0" | cut -c 3-
		exit 0
		;;
	--selftest)
		selftest || exit 1
		exit 0
		;;
	--no-sweep)
		sweep=no
		shift
		;;
	*) die "usage: gate.sh [--no-sweep] | --selftest" ;;
	esac
done

preconditions "$sweep"

run_check "the gate's own rules" "scripts/gate.sh --selftest" selftest
run_check "house style" "" scripts/check-style.sh
run_check "pull request body rules" "" scripts/check-pr-body.sh --selftest
run_check "mutation sweep rules" "" scripts/check-mutants.sh --selftest
run_check "no co-authored commits" "git log --format='%B' origin/main..HEAD" no_coauthors
run_check "formatting" "cargo fmt --all" cargo fmt --all -- --check
run_check "clippy (all features)" "" cargo clippy --all-targets --all-features
run_check "clippy (default features)" "" cargo clippy --all-targets
run_check "clippy (no default features)" "" cargo clippy --all-targets --no-default-features
run_check "documentation" "" cargo doc --no-deps --all-features
run_check "tests" "" cargo test --all-features
tests_rc="$last_rc"
run_check "cross-compile guard (aarch64)" "" \
	cargo check --target aarch64-unknown-linux-gnu --all-features

sweep_state=ran
if [ "$sweep" = no ]; then
	sweep_state=off
elif [ "$tests_rc" != 0 ]; then
	sweep_state=red
else
	note "→ scripts/check-mutants.sh"
	rc=0
	scripts/check-mutants.sh || rc=$?
	[ "$rc" = 0 ] || record "mutation coverage" "scripts/check-mutants.sh"
fi

summarise "$failures" "$sweep_state"
