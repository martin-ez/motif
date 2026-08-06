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

Each one's own module doc says what it does and how to run it. What follows is
the part of `loopback` that belongs to the repository rather than to the
program: the procedure, and the figures it produces.

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
