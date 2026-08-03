<!--
Title: type(scope): summary — under 50 characters, and clear on its own.
Body: what changed and why. Not how; the code covers that. See AGENTS.md 4.
-->

## What and why

<!-- The problem, the decision, and anything a reviewer could not infer from the
     diff: a trade-off taken, an alternative rejected, a risk accepted.
     Link the issue: Closes #N -->

## Checks that CI cannot make

CI covers the mechanical rules. These are the ones only you can confirm — see
`AGENTS.md` for the full set.

- [ ] **Tests were written first**, and failed for the right reason before the
      implementation existed (2.2)
- [ ] **Tests read as documentation** — names state behaviour, and a reader could
      learn what the code does from them alone (1.5)
- [ ] **Documentation went in the code**, not into a standalone file (1.2, 1.6)
- [ ] **No doc comments on private items** (1.3)
- [ ] **Nothing here describes behaviour that does not work yet** — planned work
      went to an issue instead (1.7)
- [ ] **This is small enough to review properly**, and the branch is a day old at
      most (3.1, 3.2)
- [ ] **Incomplete work is behind a named feature flag**, off by default (3.4)
- [ ] **None of the four design invariants moved** — retrospective analysis, a
      real-time callback, timestamps rather than BPM, UI behind an abstraction

## Verified

<!-- What you actually ran, and what you did not. Accuracy claims need a number
     against a fixture set, not an adjective. -->

## New dependencies

<!-- Delete if none. Otherwise: what it does, why not hand-rolled, its licence,
     and confirmation that it cross-compiles to aarch64. -->
