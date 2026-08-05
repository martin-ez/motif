//! Draw a representative page at the profile's screen size, to see whether the
//! size is enough.
//!
//! ```sh
//! cargo run --example layout        # takes the terminal over, centred
//! cargo run --example layout | cat  # the same grid as plain text
//! ```
//!
//! This is a ruler, not a screen the application has: whether the profile's
//! screen has room for a transport, a meter and a position readout at once
//! cannot be checked in isolation, only laid out and looked at. Everything drawn
//! comes from a type that already exists, and the keys are drawn under the box
//! rather than in it, as they sit on the device.
//!
//! In a terminal it runs through the same stack the binary does, and any control
//! leaves. Piped, it writes the grid as plain text between the same box-drawing
//! edges, which is what makes a layout readable in a diff.
//!
//! The meter's bar runs down to -48 dB: low enough that a quiet take still moves
//! it, high enough that what a converter idles at does not.

use std::io::{self, IsTerminal, Write};

use motif::audio::Levels;
use motif::device::{Button, DeviceProfile, Encoder};
use motif::looper::{LoopBuffer, Transport};
use motif::ui::{
    App, Cell, ControlEvent, EventLoop, Flow, Frame, KeyReader, Legend, Panel, RenderError,
    TerminalScreen,
};

const SCREEN: motif::device::ScreenProfile = DeviceProfile::TARGET.screen;

const METER_FLOOR_DECIBELS: f32 = -48.0;

const FILLED: char = '█';
const EMPTY: char = '░';
const PEAK: char = '┃';
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

fn captured(wanted: f32) -> LoopBuffer {
    let profile = DeviceProfile::TARGET.audio;
    let mut buffer = LoopBuffer::for_profile(profile);
    let block = vec![0.0f32; profile.block_size as usize];
    let target = (wanted * profile.sample_rate as f32) as usize;

    while buffer.len() + block.len() <= target {
        if buffer.record(&block) == 0 {
            break;
        }
    }
    buffer
}

fn draw_meter(frame: &mut Frame, row: usize, levels: Levels) {
    let readout = format!("{:>5.1} dB", decibels(levels.peak));
    let label = "IN";
    let gap = 2;
    let width = SCREEN.columns - label.len() - gap - readout.chars().count() - gap;

    let bar_at = label.len() + gap;
    let peak_cell = ((meter_fraction(levels.peak) * width as f32).round() as usize)
        .min(width.saturating_sub(1));

    write_at(frame, 0, row, label);
    write_at(frame, bar_at, row, &bar(meter_fraction(levels.rms), width));
    frame.set(bar_at + peak_cell, row, Cell::new(PEAK));
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
    let stopped = matches!(transport, Transport::Stopped);
    let indicators = format!(
        "{} REC     {} PLAY     {} STOP",
        lamp(transport.captures_input()),
        lamp(transport.plays_loop()),
        lamp(stopped),
    );

    write_at(frame, 0, row, &indicators);
}

struct Layout {
    transport: Transport,
    levels: Levels,
    loop_buffer: LoopBuffer,
}

impl Layout {
    fn new() -> Self {
        Self {
            transport: Transport::Idle.record().record(),
            levels: Levels {
                peak: 0.54,
                rms: 0.30,
            },
            loop_buffer: captured(8.0),
        }
    }

    fn draw_into(&self, frame: &mut Frame) {
        write_at(frame, 0, 0, "motif");
        write_right(frame, 0, named(self.transport));
        rule(frame, 1);

        draw_meter(frame, 3, self.levels);
        draw_position(frame, 5, &self.loop_buffer);
        draw_transport(frame, 8, self.transport);
    }

    fn page(&self) -> Frame {
        let mut frame = Frame::blank();
        self.draw_into(&mut frame);
        frame
    }

    fn panel(&self) -> Panel {
        self.legend().picture(&KeyReader::new(io::empty()))
    }
}

impl App for Layout {
    fn control(&mut self, _: ControlEvent) -> Flow {
        Flow::Exit
    }

    fn legend(&self) -> Legend {
        Legend::blank()
            .answering(Button::Play)
            .answering(Button::Stop)
            .answering(Button::Record)
            .answering(Encoder::Main)
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        self.draw_into(frame);
        Flow::Continue
    }
}

fn print_plain(frame: &Frame, panel: &Panel) -> io::Result<()> {
    let mut out = io::stdout();
    let span = String::from(RULE).repeat(SCREEN.columns);
    let margin = " ".repeat((SCREEN.columns + 2 - Panel::COLUMNS) / 2);

    writeln!(out, "┌{span}┐")?;
    for row in 0..SCREEN.rows {
        let line: String = (0..SCREEN.columns)
            .map(|column| frame.get(column, row).unwrap_or(Cell::BLANK).glyph())
            .collect();
        writeln!(out, "│{line}│")?;
    }
    writeln!(out, "└{span}┘")?;
    writeln!(out)?;

    for row in 0..Panel::ROWS {
        let keys: String = (0..Panel::COLUMNS)
            .map(|column| panel.get(column, row).unwrap_or(Cell::BLANK).glyph())
            .collect();
        writeln!(out, "{margin}{}", keys.trim_end())?;
    }

    writeln!(out, "\n{} columns x {} rows", SCREEN.columns, SCREEN.rows)
}

fn show_in_terminal(layout: &mut Layout) -> Result<(), RenderError> {
    let mut terminal = TerminalScreen::open()?;
    let (controls, mut screen) = terminal.split();

    EventLoop::new().run(layout, controls, &mut screen)?;

    Ok(())
}

fn main() -> Result<(), RenderError> {
    let mut layout = Layout::new();

    if io::stdout().is_terminal() {
        return show_in_terminal(&mut layout);
    }

    print_plain(&layout.page(), &layout.panel()).map_err(|_| RenderError::WriteFailed)
}
