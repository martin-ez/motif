//! Render the synthetic fixture set into `tests/fixtures`, one WAV and one
//! annotation per fixture.
//!
//! ```sh
//! cargo run --example generate-fixtures
//! ```
//!
//! Regenerating the set moves the benchmark every accuracy claim is measured
//! against, so it is a command someone runs on purpose rather than something an
//! unrelated change triggers. `tests/fixture_set.rs` fails while the files on
//! disk disagree with the generator, which is what makes the two stay together.

use motif::fixtures::synth;
use std::fs;
use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(&directory)?;

    for fixture in synth::set() {
        let audio = directory.join(format!("{}.wav", fixture.name()));
        let annotation = directory.join(format!("{}.beats", fixture.name()));
        fs::write(&audio, fixture.wav_bytes())?;
        fs::write(&annotation, fixture.annotation_text())?;

        println!(
            "{:<20} {:>7} bytes  {:>3} beats  {:>3} onsets  {}",
            fixture.name(),
            fs::metadata(&audio)?.len(),
            fixture.beats().len(),
            fixture.onsets().len(),
            fixture.description(),
        );
    }

    Ok(())
}
