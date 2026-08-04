//! Draw a representative page at the profile's screen size, to see whether the
//! size is enough.
//!
//! ```sh
//! cargo run --example layout        # bordered, in the terminal
//! cargo run --example layout | cat  # the same grid as plain text
//! ```
//!
//! This is a ruler, not a screen the application has. Whether 40x15 has room
//! for a transport, a meter and a position readout at once is the one number in
//! [`DeviceProfile`] that cannot be checked in isolation, and the only way to
//! answer it is to lay the three out and look. The real widgets are built
//! elsewhere; what is measured here is the space they have to fit in.
//!
//! Everything drawn comes from a type that already exists — [`Transport`] for
//! the indicators, [`Levels`] for the meter, [`LoopBuffer`] for the position —
//! so the page shows the shape of what the crate can already say, and invents
//! no behaviour to fill the frame.
//!
//! Piped, it writes the grid as plain text between the same box-drawing edges
//! the terminal would show, which is what makes the layout readable in a diff
//! or a pull request.

use std::io::{self, IsTerminal, Write};

use motif::audio::Levels;
use motif::device::{DeviceProfile, Encoder};
use motif::looper::{LoopBuffer, Transport};
use motif::ui::{Cell, Frame, RenderError, Renderer, Viewport};

const SCREEN: motif::device::ScreenProfile = DeviceProfile::TARGET.screen;

/// The quietest level the meter draws, below which the bar is empty.
const METER_FLOOR_DECIBELS: f32 = -48.0;

const FILLED: char = '█';
const EMPTY: char = '░';
const RULE: char = '─';

fn write_at(frame: &mut Frame, column: usize, row: usize, text: &str) {
    for (offset, glyph) in text.chars().enumerate() {
        frame.set(column + offset, row, Cell::new(glyph));
    }
}

fn write_right(frame: &mut Frame, row: usize, text: &str) {
    let width = text.chars().count();
    write_at(frame, SCREEN.columns.saturating_sub(width), row, text);
}

fn rule(frame: &mut Frame, row: usize) {
    write_at(frame, 0, row, &String::from(RULE).repeat(SCREEN.columns));
}

fn decibels(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        return METER_FLOOR_DECIBELS;
    }
    (20.0 * amplitude.log10()).max(METER_FLOOR_DECIBELS)
}

fn bar(fraction: f32, width: usize) -> String {
    let filled = (fraction.clamp(0.0, 1.0) * width as f32).round() as usize;
    String::from(FILLED).repeat(filled) + &String::from(EMPTY).repeat(width - filled)
}

fn meter_fraction(amplitude: f32) -> f32 {
    1.0 - decibels(amplitude) / METER_FLOOR_DECIBELS
}

fn named(transport: Transport) -> &'static str {
    match transport {
        Transport::Idle => "IDLE",
        Transport::Recording => "RECORDING",
        Transport::Playing => "PLAYING",
        Transport::Overdubbing => "OVERDUBBING",
        Transport::Stopped => "STOPPED",
    }
}

fn lamp(lit: bool) -> char {
    if lit { FILLED } else { EMPTY }
}

fn seconds(frames: usize) -> f32 {
    frames as f32 / DeviceProfile::TARGET.audio.sample_rate as f32
}

/// A loop with roughly `wanted` seconds captured, filled a block at a time the
/// way the callback fills it.
fn captured(wanted: f32) -> LoopBuffer {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.0f32; profile.block_size as usize];
    let target = (wanted * profile.sample_rate as f32) as usize;

    while buffer.len() + block.len() <= target {
        buffer.record(&block);
    }
    buffer
}

fn draw_meter(frame: &mut Frame, row: usize, levels: Levels) {
    let readout = format!("{:>5.1} dB", decibels(levels.peak));
    let label = "IN";
    let gap = 2;
    let width = SCREEN.columns - label.len() - gap - readout.chars().count() - gap;

    write_at(frame, 0, row, label);
    write_at(
        frame,
        label.len() + gap,
        row,
        &bar(meter_fraction(levels.rms), width),
    );
    write_right(frame, row, &readout);
}

fn draw_position(frame: &mut Frame, row: usize, loop_buffer: &LoopBuffer) {
    let elapsed = seconds(loop_buffer.len());
    let length = seconds(loop_buffer.capacity());

    write_at(frame, 0, row, "LOOP");
    write_at(frame, 6, row, &format!("{elapsed:>6.2} / {length:>6.2} s"));
    write_right(frame, row, &format!("{:>3.0}%", 100.0 * elapsed / length));
    write_at(frame, 0, row + 1, &bar(elapsed / length, SCREEN.columns));
}

fn draw_transport(frame: &mut Frame, row: usize, transport: Transport) {
    let stopped = !transport.captures_input() && !transport.plays_loop();
    let indicators = format!(
        "{} REC     {} PLAY     {} STOP",
        lamp(transport.captures_input()),
        lamp(transport.plays_loop()),
        lamp(stopped),
    );

    write_at(frame, 0, row, &indicators);
}

fn draw_encoders(frame: &mut Frame, row: usize) {
    let slot = SCREEN.columns / Encoder::ALL.len();

    for (position, _) in Encoder::ALL.iter().enumerate() {
        let label = format!("{} {}", position + 1, String::from(RULE).repeat(slot - 4));
        write_at(frame, position * slot, row, &label);
    }
}

fn representative_page() -> Frame {
    let transport = Transport::Idle.record().record();
    let levels = Levels {
        peak: 0.54,
        rms: 0.30,
    };
    let loop_buffer = captured(8.0);

    let mut frame = Frame::blank();

    write_at(&mut frame, 0, 0, "motif");
    write_right(&mut frame, 0, named(transport));
    rule(&mut frame, 1);

    draw_meter(&mut frame, 3, levels);
    draw_position(&mut frame, 5, &loop_buffer);
    draw_transport(&mut frame, 8, transport);

    rule(&mut frame, SCREEN.rows - 3);
    draw_encoders(&mut frame, SCREEN.rows - 2);

    frame
}

fn print_plain(frame: &Frame) -> io::Result<()> {
    let mut out = io::stdout();
    let span = String::from(RULE).repeat(SCREEN.columns);

    writeln!(out, "┌{span}┐")?;
    for row in 0..SCREEN.rows {
        let line: String = (0..SCREEN.columns)
            .map(|column| frame.get(column, row).unwrap_or(Cell::BLANK).glyph())
            .collect();
        writeln!(out, "│{line}│")?;
    }
    writeln!(out, "└{span}┘")?;
    writeln!(out, "\n{} columns x {} rows", SCREEN.columns, SCREEN.rows)
}

fn show_in_terminal(frame: &Frame) -> Result<(), RenderError> {
    let mut viewport = Viewport::new(io::stdout());
    viewport.render(frame)?;

    let mut out = io::stdout();
    let _ = write!(out, "\u{1b}[{};1Hpress enter to leave", SCREEN.rows + 4);
    let _ = out.flush();

    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    Ok(())
}

fn main() -> Result<(), RenderError> {
    let page = representative_page();

    if io::stdout().is_terminal() {
        return show_in_terminal(&page);
    }

    print_plain(&page).map_err(|_| RenderError::WriteFailed)
}
