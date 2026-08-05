#!/usr/bin/env bash
#
# Mechanical checks for the rules in AGENTS.md that rustc and clippy cannot
# express. Run by CI and safe to run locally at any time.
#
#   scripts/check-style.sh

set -euo pipefail

cd "$(dirname "$0")/.."

status=0

# AGENTS.md 1.1, in the units the rule is written in. Changing a number here
# changes the rule, so change the rule too.
ITEM_DOC_BUDGET=8
MODULE_DOC_BUDGET=12

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
	# --- AGENTS.md 1.1 — a doc comment has a budget ----------------------
	#
	# The budget is in prose lines, because that is what a reader pays: the
	# blank separators and the fenced examples are not what makes a doc
	# comment unreadable, and an example is a test (1.5), so charging for it
	# would price the wrong thing.
	#
	# Two budgets rather than one. An item doc answers what a signature does
	# and what it promises; a module doc is the only place this project puts
	# the shape of a folder, since 1.6 leaves it nowhere else to go.
	found=$(git ls-files '*.rs' | tr '\n' '\0' | xargs -0 awk '
		function report() {
			if (kind != "" && count > budget)
				printf "%s:%d: %d prose lines in a %s comment, budget %d\n",
					file, start, count, kind, budget
			kind = ""
		}
		FILENAME != seen { report(); seen = FILENAME }
		{
			if ($0 !~ /^[ \t]*\/\/[\/!]/) { report(); next }
			k = ($0 ~ /^[ \t]*\/\/!/) ? "//!" : "///"
			if (k != kind) {
				report()
				kind = k; file = FILENAME; start = FNR
				count = 0; fenced = 0
				budget = (k == "//!") ? '"$MODULE_DOC_BUDGET"' : '"$ITEM_DOC_BUDGET"'
			}
			text = $0
			sub(/^[ \t]*\/\/[\/!]/, "", text)
			gsub(/^[ \t]+|[ \t]+$/, "", text)
			if (text ~ /^```/) { fenced = !fenced; next }
			if (fenced || text == "") next
			count++
		}
		END { report() }
	' || true)
	if [ -n "$found" ]; then
		report "doc comment over budget (AGENTS.md 1.1)" "$found
A doc comment says what the item does and what it promises, then stops. Past
that budget it is describing the implementation, and the implementation is
already there to read. Cut it to the contract, or make the code carry what the
prose was carrying — a name, or a function whose signature says it instead."
	else
		pass "no doc comment over budget (1.1)"
	fi

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
# .github/ are configuration, and so are agent skills under .claude/ — a skill
# is a procedure that fails when it goes stale, not prose that is believed.
found=$(git ls-files '*.md' '*.markdown' |
	grep -vE '^(README|AGENTS|CLAUDE|CONTRIBUTING)\.md$' |
	grep -vE '^\.github/' |
	grep -vE '^\.claude/' |
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
#
# An issue number on its own is not the signal. A doc comment naming the issue
# that owns a case the code deliberately does not handle is describing today's
# boundary, and src/ui.rs does exactly that. What turns a reference into prose
# about the future is the promise beside it: a verb saying the issue changes
# this code, or a clause holding until it lands. Those are what `anchored`
# matches, so a bare `[#193]` passes and `#216 replaces this` does not.
phrases='coming soon|not yet implemented|will be implemented|planned for a|in a future release|once implemented|for now, this is a placeholder'
anchored='#[0-9]+ *(replaces|supersedes|removes|rewrites|will|lands\b)|(replaced|superseded|removed|rewritten|handled|fixed) (by|in) #[0-9]+|\b(until|once|when) #[0-9]+'
targets=$(git ls-files '*.md' '*.rs' |
	grep -vE '^(AGENTS\.md|CLAUDE\.md|\.github/pull_request_template\.md)$' || true)
if [ -n "$targets" ]; then
	found=$(printf '%s\n' "$targets" | tr '\n' '\0' |
		xargs -0 grep -rniE "$phrases|$anchored" 2>/dev/null || true)
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
