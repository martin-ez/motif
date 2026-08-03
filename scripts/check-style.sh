#!/usr/bin/env bash
#
# Mechanical checks for the rules in AGENTS.md that rustc and clippy cannot
# express. Run by CI and safe to run locally at any time.
#
#   scripts/check-style.sh

set -euo pipefail

cd "$(dirname "$0")/.."

status=0

report() {
	printf '\n\033[31mFAIL\033[0m  %s\n' "$1"
	printf '%s\n' "$2" | sed 's/^/      /'
	status=1
}

pass() {
	printf '\033[32mok\033[0m    %s\n' "$1"
}

roots=()
for dir in src tests benches examples; do
	[ -d "$dir" ] && roots+=("$dir")
done

# --- AGENTS.md 2.1 — tests exercise only the public API ----------------------
#
# Tests belong in tests/, where they compile as an external consumer of the
# crate and the compiler makes private items unreachable. A #[cfg(test)] module
# inside src/ can reach private items, which turns implementation details into
# things the tests depend on.
found=$(grep -rn --include='*.rs' '#\[cfg(test)\]' src 2>/dev/null || true)
if [ -n "$found" ]; then
	report "unit test module inside src/ (AGENTS.md 2.1)" "$found
Move these to tests/. Tests there can only reach the public API, which is
the point: the implementation stays free to change."
else
	pass "no test modules in src/ (2.1)"
fi

if [ ${#roots[@]} -gt 0 ]; then
	# --- AGENTS.md 1.4 — no inline comments ------------------------------
	#
	# /// and //! are documentation and are allowed. // is not.
	#
	# Both exclusions have to skip past grep's own "file:line:" prefix. A
	# leading // comment turns that prefix into the literal substring
	# ":8://", so a naive URL filter silently discards every comment at the
	# start of a line — which is most of them.
	found=$(grep -rnE --include='*.rs' '//' "${roots[@]}" 2>/dev/null |
		grep -vE '^[^:]+:[0-9]+:[[:space:]]*(///|//!)' |
		grep -vE '^[^:]+:[0-9]+:.*://' || true)
	if [ -n "$found" ]; then
		report "inline comment (AGENTS.md 1.4)" "$found
Delete it, or make the code say it: name the value, extract the step into a
function whose name is the sentence you were about to write. Where the fact is
genuinely not derivable from the code — a threshold from a paper, a constant
measured on real audio — it belongs in the doc comment of the public item that
uses it, so that a reader of \`cargo doc\` sees it too."
	else
		pass "no inline comments (1.4)"
	fi

	# --- AGENTS.md 3.4 — hidden work sits behind a named feature ----------
	found=$(grep -rnE --include='*.rs' '\b(TODO|FIXME|XXX|HACK)\b' "${roots[@]}" 2>/dev/null || true)
	if [ -n "$found" ]; then
		report "TODO marker (AGENTS.md 3.4)" "$found
Incomplete work goes behind a Cargo feature and into a GitHub issue, where it
is visible and can block other work. A marker in a source file is neither."
	else
		pass "no TODO markers (3.4)"
	fi
fi

# --- AGENTS.md 1.6 — the only prose outside the code is a folder README ------
#
# Four root files are exempt because they are the project's contract rather
# than documentation of it, and GitHub looks for them by name. Templates under
# .github/ are configuration.
found=$(git ls-files '*.md' '*.markdown' |
	grep -vE '^(README|AGENTS|CLAUDE|CONTRIBUTING)\.md$' |
	grep -vE '^\.github/' |
	grep -vE '(^|/)README\.md$' || true)
if [ -n "$found" ]; then
	report "documentation outside the code that is not a folder README (AGENTS.md 1.6)" "$found
Rename it to README.md in the folder it describes, or move it into a doc
comment on the code it explains. A folder gets one README; it does not get a
library."
else
	pass "prose outside the code is folder READMEs only (1.6)"
fi

# A folder README describes the folder it sits in. A folder holding nothing but
# prose is a documentation tree wearing a README's name, which is the thing 1.6
# exists to prevent.
found=$(git ls-files '*/README.md' | while read -r readme; do
	siblings=$(git ls-files "${readme%/README.md}" |
		grep -vE '\.(md|markdown)$' | head -1)
	if [ -z "$siblings" ]; then
		printf '%s\n' "$readme"
	fi
done)
if [ -n "$found" ]; then
	report "a folder containing only documentation (AGENTS.md 1.6)" "$found
This folder holds no code, so its README describes something other than itself —
which makes it a document, not a folder README. Move what is still true into the
code or the top-level README, and the rest into issues."
else
	pass "every folder README sits beside code (1.6)"
fi

# --- AGENTS.md 1.7 — documentation describes what exists ---------------------
#
# AGENTS.md and the pull request template state this rule and so must be able
# to name the thing they forbid. CLAUDE.md is a symlink to AGENTS.md, and GNU
# grep follows symlinks named on the command line where BSD grep does not — so
# leaving it out here passes on macOS and fails on Linux.
targets=$(git ls-files '*.md' '*.rs' |
	grep -vE '^(AGENTS\.md|CLAUDE\.md|\.github/pull_request_template\.md)$' || true)
if [ -n "$targets" ]; then
	found=$(printf '%s\n' "$targets" | tr '\n' '\0' |
		xargs -0 grep -rniE 'coming soon|not yet implemented|will be implemented|planned for a|in a future release|once implemented|for now, this is a placeholder' 2>/dev/null || true)
	if [ -n "$found" ]; then
		report "documentation of something that does not exist (AGENTS.md 1.7)" "$found
Describe what the code does today. Work that has not happened belongs in a
GitHub issue, where it is queryable and can block other work — not in prose
that no test and no compiler will ever contradict."
	else
		pass "no documentation of unbuilt features (1.7)"
	fi
fi

exit $status
