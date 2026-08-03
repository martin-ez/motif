# Project Brief — Terminal Groovebox

## What it is

A terminal-based music groovebox that listens to you play, works out the musical structure of
what you played, and uses that understanding to help you build a song sketch.

You play. It captures a loop, correctly aligned to bar one. It tells you the tempo, key,
chord progression, and notes. You overdub, or let it generate parts that fit.

The goal is a fast, fun sketchpad — not a DAW. The terminal is a deliberate constraint: it
removes UI design from the critical path and forces the interaction to stay simple.

Long-term ambition: a standalone hardware device with a small screen.

## Why it doesn't already exist

The pieces exist separately. Nothing combines them.

- **DigiTech Trio+** (2015) is the closest: it listens to a guitar, infers key, tempo, and
  chords, and generates bass and drums. But it's a closed appliance — guitar only, 3/4 and 4/4
  only, canned genre templates, no MIDI out, and no access to the metadata it derived.
- **Ableton Live** has tempo following, but tempo only. No downbeat, no harmony.
- **Offline analysers** (`allin1`, Beat This!, Basic Pitch, HoRNet SongKey) are accurate but
  file-based and disconnected from any looper.
- **Terminal DAWs** (Tek, `soundlooper`, `rtrack`) exist and work, but do no analysis.

The gap: analysis-driven loop capture, with the metadata as a first-class, visible, editable
object that drives generation — in an open, hackable, instrument-agnostic tool.

## The core architectural insight

**Latency tolerance buys accuracy.**

Online (causal, sub-50ms) beat tracking is decent, but online *downbeat* tracking is poor —
current state of the art reaches roughly 47–53% F1. Offline models on the same task reach
75–80%. Downbeat is exactly what determines where bar one is, and therefore where the loop
cuts.

So: analyse retrospectively, not causally. The loop is captured first, then analysed. This is
not a compromise — it is the higher-accuracy path, and it buys roughly 25–30 percentage points
on the metric that matters most.

Because loop length is set in advance, the system never needs a causal model at all. It has a
*deadline*, not a latency budget: it must decide where the loop closes before that moment
arrives, which at 4 bars is several seconds of thinking time.

## How it works

| Layer | Timing | Role |
|---|---|---|
| Monitoring | Zero latency | Passthrough; always recording to a rolling pre-roll buffer |
| Provisional tempo | Deadline-scheduled | Enough to schedule the loop close accurately |
| Full analysis | 1–3s after capture | Downbeat, key, chords, audio-to-MIDI at offline quality |
| Re-alignment | On completion | Adjust loop boundaries and metadata against the stored buffer |

The **rolling pre-roll buffer** is essential. It lets the loop start be moved *earlier* than
the moment the user pressed the button, which is what makes correct downbeat detection feel
like magic rather than merely accurate.

## Simplifying decisions

**Single instrument in.** No source separation. Puts audio-to-MIDI in its documented sweet
spot and makes chord detection tractable. Trade-off accepted: no drums means beat tracking
runs on the harder, non-percussive case.

**User sets loop length in bars.** This is a strong prior, not just a convenience. Combined
with a plausible BPM range it prunes most half/double-time errors before they happen, and once
the loop closes, tempo becomes *derivable exactly* (duration ÷ beat count) rather than
estimated. Beat tracking becomes constrained fitting against a known count — a materially
easier problem than the open-ended benchmarks measure.

**Steady-ish tempo assumed** in the estimator, but never in the data model.

## Known risks

1. **Half/double-time errors** are the one unrecoverable failure — a wrong tempo with fixed
   bar count gives a loop of the wrong duration. Phase errors, by contrast, are fixable after
   the fact by rotating the buffer. Mitigations: the bar-count prior, the fact that only the
   first loop is hard, and a one-keystroke ×2 / ÷2 / rotate escape hatch.
2. **Chord detection on dense material** is the weakest analyser. Expose confidence; let the
   user correct.
3. **Metadata mutating under the user's hands** as analysis settles. Needs an explicit policy:
   re-derive live until the first generated part exists, then freeze with an explicit
   re-analyse action.
4. **Loop seam** — wrapping from a settled end-tempo to an unsettled start-tempo. Crossfade.

## Scope

**v1:** Passthrough, loop capture, beat grid, tempo, downbeat. One generated MIDI part.

**Later:** Chords, key, audio-to-MIDI, multi-layer overdubs, song sections.

**Deferred:** Source separation, rubato / warp maps, polyphonic multi-instrument input.

## Stack

Single-language Rust, monolithic. Full detail in `bootstrap-brief.md`.

The load-bearing decisions: everything cross-compiles to aarch64; the audio callback is
strictly real-time; the UI renders through an abstraction rather than to a terminal; the beat
grid is an array of timestamps, not a BPM scalar.

---

## Note on `bootstrap-brief.md`

The stack document referenced above does not exist in this repository yet. A file named
`audio-analysis-tool-tech-stack.md` (March 2025) sits outside the repo and is **superseded**:
it describes a Flutter + `flutter_rust_bridge` UI, real-time *causal* analysis, and a CLAP
plugin wrapper — all of which this brief overrides. Treat it as background only. Crate
choices are made per-issue until a real stack document lands.
