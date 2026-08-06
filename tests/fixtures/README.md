# Fixtures

Annotated audio to measure an analyser against. Every file here is written by
`motif::fixtures::synth`, and every claim about accuracy in this repository is a
number measured over this set.

Each fixture is two files sharing a name: `<name>.wav` is 8-bit mono PCM at
8 kHz, and `<name>.beats` is its ground truth in the format
`motif::fixtures::Annotation` documents. The beats are where the pulse is, which
is not always where the sounds are.

Eight bits rather than sixteen is what pays for four bars a fixture inside the
size ceiling, and four bars is what the accuracy figures rest on: a tracker that
misreads one bar moves the aggregate by a thirty-sixth. Against clicks that peak
near full scale, the quantisation noise it costs sits far below anything an
onset envelope resolves.

Most fixtures annotate rhythm alone. `chords-150-4-4` also annotates a chord to
the bar, and `line-150-4-4` the notes of a monophonic line; both are played on
pitched voices with no percussion, so a chroma or pitch front-end hears the
harmony rather than a noise burst over it. A fixture annotating neither is not
part of a `measure_chords` or `measure_notes` run at all, rather than scoring
zero on something it never claimed.

The audio is synthetic, so its ground truth is exact by construction rather than
tapped by a human, and its licence is this repository's. The standard annotated
corpora — Ballroom, Hainsworth, GTZAN-rhythm — carry terms that rule out
vendoring their audio into a public repository; they can be run locally by
anyone who has them, and are never committed here.

## The deadline

Accuracy is half of what a run over this set reports. The other half is time:
the loop wraps to bar one and plays again the moment the take closes, so
analysis is bounded by the loop the player set rather than by a fixed figure.
`motif::fixtures::harness::deadline` takes it as a share of the take, which
gives every fixture here a deadline of its own, and a report carries what each
fixture took beside its F1 and the tightest headroom the set had left.

The figure that binds is the one measured on the target board. A development
machine is the optimistic case — that is what the `aarch64` corollary in
`AGENTS.md` means — so headroom measured on a laptop is not a figure to quote.
Run the harness over this set on the board, with the analyser under test as the
candidate, and record the tightest headroom it reports:

| Date | Board | Candidate | Tightest headroom |
| ---- | ----- | --------- | ----------------- |

No figure has been recorded. Nothing in the repository analyses audio, so the
only candidate the harness can be handed is one that reads the answers off these
files, and what that times is the harness itself.

## Regenerating

```sh
cargo run --example generate-fixtures
```

Do this on purpose and never as a side effect: regenerating moves the benchmark,
which silently moves every number measured against it. `tests/fixture_set.rs`
fails while these files disagree with the generator, and holds the set under its
576 KiB ceiling.
