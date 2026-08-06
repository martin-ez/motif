# examples

Things to run against real hardware, which `cargo test` cannot reach: a device
that has to be plugged in, a layout that has to be looked at, a fixture set
whose regeneration moves the benchmark every accuracy claim is measured
against.

- `duplex` — open a duplex stream on the default devices, print what they
  granted, and pass the input through to the output.
- `layout` — draw a representative page at the target profile's screen size, to
  see whether that size is enough.
- `generate-fixtures` — render the synthetic fixture set into `tests/fixtures`.
- `loopback` — time the round trip out of the output and back into the input.
- `crossing` — time a finished take crossing off the audio thread.

Each one's own module doc says what it does and how to run it. What follows is
the part of `loopback` and `crossing` that belongs to the repository rather than
to the programs: the procedures, and the figures they produce.

## Measuring the round trip

"Zero-latency passthrough" is a claim about a number. This is how the number is
obtained, so that a later change adding a buffer somewhere is caught by a figure
rather than by feel.

1. Wire the output back to the input. A jack lead between line out and line in
   on an interface; failing that, the headphone socket to the microphone socket.
   A speaker and a microphone will also do, and the figure then carries the air
   between them, which is about 3 ms per metre.
2. Set the input gain so a full-scale click comes back below clipping and well
   above the noise floor. Anything from about a fifth of full scale up is
   detected.
3. Run it, and press Enter when asked.

```sh
cargo run --example loopback
```

It clicks up to nine times, a quarter of a second apart, and reports each round
trip in frames and milliseconds along with the median. The click is one frame at
full scale, so turn the output down before running it into anything you are
listening to.

The first click that does not come back within a quarter of a second stops it,
rather than starting another take. That is what keeps the figure honest: a
second click would collect the first one's late return inside its own window and
report a slow loop as a fast one. So a loop that is not wired up, and one slower
than the listening window, both print nothing rather than a wrong number — which
is the case for using a cable rather than a wireless output.

## The budget

**Five blocks**, which at the target profile's 48 kHz in 256-frame blocks is
1280 frames, or **26.7 ms**. It is stated in blocks rather than milliseconds so
that it follows the block size the stream was opened for.

The block it follows is the one **requested**, not the one the device granted. A
device may grant a shorter block, and the boundary's slack is built from the
request either way, so a budget denominated in the granted size would shrink
while the thing it is paying for did not.

Where the five come from:

| Blocks | What they pay for                                                |
| -----: | ---------------------------------------------------------------- |
|      1 | the boundary's own slack, the least that keeps playback from outrunning capture |
|      2 | the device's conventional double buffer on capture                |
|      2 | the same on playback                                             |

The budget is a structural derivation rather than a measurement, so the first
figures taken over a cable are what confirm or correct it. `RoundTrip::budget`
holds the number, and the example prints whether the median is inside it.

## Measurements

| Date | Machine | Interface | Loop | Granted | Median | Within budget |
| ---- | ------- | --------- | ---- | ------- | ------ | ------------- |

No run has been recorded. A run adds a row, with the median from the output
above.

## Measuring the take crossing

A finished take crosses to the analysis thread inside the audio callback, a
share of it a block. What that share costs is a claim about a number too, and
the callback is where a number that is wrong costs a dropout rather than a
slower answer.

```sh
cargo run --release --example crossing
```

No hardware is needed and nothing is heard. It builds the worst case the target
profile allows — the longest loop, with every layer of the stack laid over it,
so a block mixes as many layers as it ever will — crosses it a block at a time,
and reports the median and worst block against the block period.

Run it on the target board rather than on a development machine. The `aarch64`
corollary makes the laptop the optimistic case, and the whole point of the
figure is that it is the one that binds. `--release` matters as much: an
unoptimised build of a copy loop measures the build.

### The budget

**A quarter of the block period**, which at the target profile's 48 kHz in
256-frame blocks is 1.33 ms. It is stated as a share rather than a duration so
that it follows the block the device granted, which is also what the share
itself now follows.

A quarter rather than the whole because the crossing is not the only thing in
the callback: the same block is gained, mixed against the loop, summarised for
the waveform and metered, and the crossing is the one piece of it that is
background work. Leaving the rest of the block three times the room the crossing
takes is what keeps a take handed over from being the reason a block is late.

The span the crossing takes is a separate figure and not a measured one. It is
`TakeWriter::CROSSING_BLOCKS` block periods — 341 ms at the target profile,
whatever block the device granted — and the example prints it as a share of the
deadline analysis has, because it is spent before analysis can start.

### Measurements

| Date | Machine | Layers | Granted | Median | Worst | Within budget |
| ---- | ------- | ------ | ------- | ------ | ----- | ------------- |

No run on the target has been recorded. A run adds a row, with the median and
worst from the output above.
