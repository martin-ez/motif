# Fixtures

Annotated audio to measure an analyser against. Every file here is written by
`motif::fixtures::synth`, and every claim about accuracy in this repository is a
number measured over this set.

Each fixture is two files sharing a name: `<name>.wav` is 16-bit mono PCM at
8 kHz, and `<name>.beats` is its ground truth in the format
`motif::fixtures::Annotation` documents. The beats are where the pulse is, which
is not always where the sounds are.

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

## Regenerating

```sh
cargo run --example generate-fixtures
```

Do this on purpose and never as a side effect: regenerating moves the benchmark,
which silently moves every number measured against it. `tests/fixture_set.rs`
fails while these files disagree with the generator, and holds the set under its
640 KiB ceiling.
