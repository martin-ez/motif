//! The device profile: the frozen shape of the machine `motif` is built for,
//! and the arithmetic the rest of the crate sizes itself with.

use motif::device::{AudioProfile, DeviceProfile, ScreenProfile};

fn target() -> DeviceProfile {
    DeviceProfile::TARGET
}

fn default_terminal() -> ScreenProfile {
    ScreenProfile {
        columns: 80,
        rows: 24,
    }
}

#[test]
fn a_screen_holds_one_cell_per_column_and_row() {
    let screen = ScreenProfile {
        columns: 4,
        rows: 3,
    };

    assert_eq!(screen.cells(), 12);
}

#[test]
fn a_screen_is_counted_in_cells_at_compile_time() {
    const CELLS: usize = DeviceProfile::TARGET.screen.cells();

    assert_eq!(CELLS, target().screen.columns * target().screen.rows);
}

#[test]
fn a_loop_is_as_many_frames_as_it_is_seconds_of_sample_rate() {
    let audio = AudioProfile {
        sample_rate: 1_000,
        block_size: 100,
        max_loop_seconds: 7,
    };

    assert_eq!(audio.max_loop_frames(), 7_000);
}

#[test]
fn the_target_screen_fits_in_a_default_terminal() {
    let (screen, terminal) = (target().screen, default_terminal());

    assert!(screen.columns <= terminal.columns);
    assert!(screen.rows <= terminal.rows);
}

#[test]
fn the_target_offers_every_kind_of_control() {
    let controls = target().controls;

    assert!(controls.encoders > 0);
    assert!(controls.buttons > 0);
}

#[test]
fn the_target_block_size_is_a_power_of_two() {
    let audio = target().audio;

    assert!(audio.block_size.is_power_of_two());
}

#[test]
fn the_target_loop_buffer_is_a_whole_number_of_blocks() {
    let audio = target().audio;

    assert_eq!(audio.max_loop_frames() % audio.block_size as usize, 0);
}

#[test]
fn the_target_has_a_core_to_spare_beside_the_audio_callback() {
    let cores = target().cores;

    assert!(cores >= 2);
}
