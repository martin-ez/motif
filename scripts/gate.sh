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
# Four of pr.yml's assertions are outside this gate, because a working tree
# cannot answer them: the title's length, its `type(scope): summary` shape,
# the body's wrapping, and the scan for tool attribution in the body. All four
# belong to a request that does not exist while this runs.
# `scripts/check-pr-body.sh -F body.md` answers the wrapping one against a
# drafted body and nothing else; the other three are yours to hold as you
# write them (4.2, 4.4, 4.5). The commit half of the attribution scan is here,
# because commits do exist.
#
# --selftest holds CI to that list rather than trusting it to hold still: it
# reads the workflows and fails on a command CI runs that this gate neither
# runs nor names in `ci_exceptions`. The body's wrapping is the one of the four
# that reaches a workflow as a command, so it is the one entry in that table;
# the other three are inline shell that no extraction can see.
#
# Every check runs even after an earlier one fails, as `!cancelled()` makes
# them in CI, so one run says everything rather than the first thing. The
# mutation sweep is last and is the exception: it decides nothing on a tree
# whose tests already fail, so a red tree skips it rather than reporting it.
#
# The heartbeat naming each command goes to stderr and every verdict to
# stdout, so `scripts/gate.sh 2>/dev/null` is the report on its own. A missing
# precondition is the exception: it stops the gate before there is a report,
# and says why on stderr.
#
# Exit codes:
#   0  every check that ran passed. A run that skipped the sweep lands here
#      too, so --no-sweep can exit 0 — the shout saying so is on stdout, where
#      output pasted into a report carries it
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

TESTS_CHECK="tests"

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
	if [ "$sweep" = red ]; then
		return 1
	fi
	if [ "$sweep" = ran ]; then
		pass "every check CI can fail a pull request on passed"
	else
		pass "every check that ran passed"
	fi
	return 0
}

# Whether a named check recorded a failure. The sweep's decision turns on the
# tests rather than on whatever happened to run last, so it asks by name and
# stops depending on where it sits among the calls.
recorded() {
	local failures="$1" want="$2" name
	while IFS="$TAB" read -r name _; do
		if [ "$name" = "$want" ]; then
			return 0
		fi
	done <<EOF
$failures
EOF
	return 1
}

# The sweep is the only check whose verdict depends on another's. It asks
# whether a diff's tests constrain behaviour, so a tree whose tests already
# fail cannot be asked, and --no-sweep says not to ask. Both are states of
# their own: a sweep that did not run has caught nothing.
sweep_state() {
	if [ "$1" = no ]; then
		printf 'off\n'
	elif recorded "$2" "$TESTS_CHECK"; then
		printf 'red\n'
	else
		printf 'ran\n'
	fi
}

# The sweep is invoked in two places that must agree — ci.yml runs it and
# scripts/check-mutants.sh runs it — and nothing else compares them. Both jobs
# inherit -D warnings, under which a mutant that trips a lint is reported
# unviable rather than tested: measured on src/ui/hold.rs, 10 of 11 mutants
# were caught with --cap-lints=true and 5 of 11 without. A flag present at one
# call site and not the other silently halves the sweep on whichever side lost
# it, which is the drift that produced it in the first place.
unpinned_sweeps() {
	local file line found=""
	for file in "$@"; do
		while IFS= read -r line; do
			[ -n "$line" ] || continue
			case "$line" in
			*--cap-lints=true*) ;;
			*) found="$found$file: $line
" ;;
			esac
		done <<EOF
$(grep -h -e 'cargo mutants --in-diff' "$file" 2>/dev/null || true)
EOF
	done
	printf '%s' "$found"
}

# A command reduced to its shape: every word after the first that names a path,
# an expansion or a count becomes a placeholder. CI writes the sweep's diff to a
# file it names and passes a job count it fixes, where scripts/check-mutants.sh
# derives both, so the two invocations agree in their flags and differ in their
# operands. Comparing shapes is what lets the wrapper account for the command it
# wraps, while a flag only one side carries still reads as a difference.
command_shapes() {
	awk '
		{
			sub(/^[[:space:]]+/, "")
			sub(/[[:space:]][0-9]*>.*$/, "")
		}
		$1 == "cargo" || $1 ~ /^scripts\// {
			$1 = $1
			for (i = 2; i <= NF; i++)
				if (index($i, "/") || index($i, "$") || $i ~ /^[0-9]+$/)
					$i = "_"
			print
		}
	'
}

# Every command a workflow runs. A line's content — after a leading `- ` and
# `run: `, and one pipeline segment at a time — is a command when it begins
# `cargo ` or `scripts/`, which reads a step inside a `run: |` block as well as
# a one-line `run:`. `uses:`, `sudo apt-get` and `git diff` steps fall out on
# their own, because they begin with something else.
ci_commands() {
	local file
	for file in "$@"; do
		tr '|' '\n' <"$file" | sed -e 's/^[[:space:]]*//' -e 's/^- //' \
			-e 's/^run: //' | command_shapes
	done
}

# The commands the gate runs, read out of the calls that run them. A check the
# gate spells as a shell function has no command line and announces the line
# that reproduces it instead, so a `run_check` call yields both its command and
# that line. Reading the calls, and a bare invocation at the top level or one
# level inside it, is what stops a command named in a comment or in a fixture
# below from accounting for one the gate stopped running.
gate_commands() {
	local file calls
	for file in "$@"; do
		calls="$(grep '^run_check ' "$file" || true)"
		{
			printf '%s\n' "$calls" | sed 's/^run_check "[^"]*" "[^"]*" //'
			printf '%s\n' "$calls" |
				sed -n 's/^run_check "[^"]*" "\([^"]*\)".*/\1/p'
			grep -E "^${TAB}?(cargo |scripts/)" "$file" || true
		} | tr '|' '\n' | command_shapes
	done
}

# Commands CI runs that the gate cannot, each with the reason it is out. A
# working tree holds no pull request, so a check that reads one is a property of
# the request rather than of the tree. The reason sits beside the command as
# data rather than in prose about it, so a reviewer sees what was excluded and
# why in the diff that excludes it.
ci_exceptions() {
	printf '%s\n' \
		"scripts/check-pr-body.sh${TAB}reads a pull request body, which does not exist while the gate runs"
}

# The commands CI runs that the gate neither runs nor excepts. Both sides are
# read out of the files that run them, so a check dropped from the gate stops
# accounting for the step in CI it mirrored.
unaccounted() {
	local ci="$1" gate="$2" exceptions="$3" cmd found=""
	while IFS= read -r cmd; do
		[ -n "$cmd" ] || continue
		if printf '%s\n' "$gate" | grep -qxF "$cmd"; then
			continue
		fi
		if printf '%s\n' "$exceptions" | cut -d"$TAB" -f1 | grep -qxF "$cmd"; then
			continue
		fi
		found="$found$cmd
"
	done <<EOF
$ci
EOF
	printf '%s' "$found"
}

# cpal's Linux backend links against ALSA, and alsa-sys resolves it through
# pkg-config from a build script, which runs even under `cargo check`. macOS
# uses CoreAudio and needs nothing installed.
needs_alsa() {
	if [ "$1" = Linux ]; then
		printf 'yes\n'
	else
		printf 'no\n'
	fi
}

# Named before anything runs rather than eight minutes into the gate, and with
# the line that installs the missing piece.
preconditions() {
	rustup target list --installed 2>/dev/null |
		grep -qx aarch64-unknown-linux-gnu ||
		die "the aarch64 target is not installed, so the cross-compile guard
      cannot run.

        rustup target add aarch64-unknown-linux-gnu"

	[ "$(needs_alsa "$(uname -s)")" = no ] || pkg-config --exists alsa 2>/dev/null ||
		die "the ALSA development headers are not installed, so cpal's Linux
      backend cannot build and every cargo check below will die inside
      alsa-sys rather than in this crate.

        sudo apt-get install -y libasound2-dev"

	[ "$1" = no ] || cargo mutants --version >/dev/null 2>&1 ||
		die "cargo-mutants is not installed, so the sweep cannot run.

        cargo install --locked cargo-mutants"
}

# A check written as a shell function has no command line of its own, so the
# heartbeat and the summary both show the line that reproduces it instead. A
# function name announced as though it were a command is one a reader cannot run.
run_check() {
	local name="$1" fix="$2" out rc=0 shown
	shift 2
	shown="$*"
	if [ "$(type -t "$1")" = function ] && [ -n "$fix" ]; then
		shown="$fix"
	fi
	note "→ $shown"
	out="$("$@" 2>&1)" || rc=$?
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

st_check_passes() { return 0; }

st_check_fails() {
	printf 'the tool said what was wrong\n'
	return 1
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
	st_has "$out" "every check CI can fail" "a clean gate says the whole gate passed"

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
	out="$(summarise "" red)" || rc=$?
	st_is 1 "$rc" "a skipped sweep is never a pass"
	st_hasnt "$out" "passed" "a tree that skipped the sweep is not reported as green"

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

	out="$(run_check "a demo" "" st_check_passes 2>&1)"
	st_has "$out" "a demo" "a passing check is named"
	st_hasnt "$out" "FAIL" "a passing check is not reported as one"

	out="$(run_check "a demo" "the line that reproduces it" st_check_passes 2>&1)"
	st_has "$out" "the line that reproduces it" \
		"a check written as a function announces a line that can be run"
	st_hasnt "$out" "st_check_passes" "the function name is not offered as a command"

	out="$( { failures=""
		run_check "a demo" "" st_check_fails
		summarise "$failures" ran
	} 2>&1 )" || true
	st_has "$out" "the tool said what was wrong" "a failing check shows what it said"
	st_has "$out" "1 check failed" "a failing check reaches the summary"
	st_has "$out" "        st_check_fails" \
		"a failure with no fix renders the command itself in the summary"

	out="$( { failures=""
		run_check "the first demo" "the first line" st_check_fails
		run_check "the second demo" "the second line" st_check_fails
		summarise "$failures" ran
	} 2>&1 )" || true
	st_has "$out" "2 checks failed" "two failing checks are recorded as two"
	st_has "$out" "        the first line" "the summary renders the first"
	st_has "$out" "        the second line" "the summary renders the second"

	st_is ran "$(sweep_state yes "")" "a green tree sweeps"
	st_is red "$(sweep_state yes "tests${TAB}cargo test --all-features")" \
		"failing tests skip the sweep"
	st_is ran "$(sweep_state yes "formatting${TAB}cargo fmt --all")" \
		"another check's failure does not skip the sweep"
	st_is off "$(sweep_state no "")" "--no-sweep skips the sweep"
	st_is off "$(sweep_state no "tests${TAB}cargo test --all-features")" \
		"--no-sweep skips it on a red tree too"

	st_is yes "$(needs_alsa Linux)" "a Linux host needs the ALSA headers"
	st_is no "$(needs_alsa Darwin)" "macOS uses CoreAudio and needs nothing"

	st_is "" "$(unpinned_sweeps .github/workflows/ci.yml scripts/check-mutants.sh)" \
		"both sweeps pin --cap-lints"

	local pinned unpinned
	pinned="$(mktemp "${TMPDIR:-/tmp}/motif-gate.XXXXXX")"
	unpinned="$(mktemp "${TMPDIR:-/tmp}/motif-gate.XXXXXX")"
	printf 'cargo mutants --in-diff "$d" --no-shuffle --cap-lints=true -j 4\n' >"$pinned"
	printf 'cargo mutants --in-diff "$d" --no-shuffle -j 4\n' >"$unpinned"

	st_is "" "$(unpinned_sweeps "$pinned")" "a pinned invocation is not reported"
	st_has "$(unpinned_sweeps "$unpinned")" "$unpinned" \
		"an invocation that dropped the flag is named"
	st_has "$(unpinned_sweeps "$pinned" "$unpinned")" "$unpinned" \
		"one call site dropping it is caught beside one that kept it"
	rm -f "$pinned" "$unpinned"

	local workflow
	workflow="$(mktemp "${TMPDIR:-/tmp}/motif-gate.XXXXXX")"
	cat >"$workflow" <<'EOF'
jobs:
  demo:
    steps:
      - uses: actions/checkout@v4
      - run: sudo apt-get update && sudo apt-get install -y libasound2-dev
      - run: cargo test --all-features
      - run: |
          git diff origin/main...HEAD > /tmp/pr.diff
          cargo bench --all-features
EOF

	out="$(ci_commands "$workflow")"
	st_has "$out" "cargo test --all-features" "a one-line run: step is a command"
	st_has "$out" "cargo bench --all-features" \
		"a cargo line after the first of a run: block is one too"
	st_hasnt "$out" "actions/checkout" "a uses: step is not a command"
	st_hasnt "$out" "apt-get" "an apt-get step is not a command"
	st_hasnt "$out" "git diff" "a git diff step is not a command"

	out="$(unaccounted "$out" "$(gate_commands scripts/gate.sh)" "$(ci_exceptions)")"
	st_has "$out" "cargo bench --all-features" \
		"a command the gate neither runs nor excepts is unaccounted"
	st_hasnt "$out" "cargo test" "a command the gate runs is accounted for"
	rm -f "$workflow"

	st_has "$(unaccounted "scripts/check-pr-body.sh" "" "")" \
		"scripts/check-pr-body.sh" \
		"dropping an entry from the exception table unaccounts its command"
	st_is "" "$(unaccounted "scripts/check-pr-body.sh" "" "$(ci_exceptions)")" \
		"the entry that carries a reason is what accounts for it"

	st_is "" "$(unaccounted \
		"$(ci_commands .github/workflows/ci.yml .github/workflows/pr.yml)" \
		"$(gate_commands scripts/gate.sh scripts/check-mutants.sh)" \
		"$(ci_exceptions)")" \
		"every command CI runs is one the gate runs or excepts"

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
run_check "$TESTS_CHECK" "" cargo test --all-features
run_check "cross-compile guard (aarch64)" "" \
	cargo check --target aarch64-unknown-linux-gnu --all-features

state="$(sweep_state "$sweep" "$failures")"
if [ "$state" = ran ]; then
	note "→ scripts/check-mutants.sh"
	rc=0
	scripts/check-mutants.sh || rc=$?
	[ "$rc" = 0 ] || record "mutation coverage" "scripts/check-mutants.sh"
fi

summarise "$failures" "$state"
