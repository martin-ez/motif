//! The terminal implementation of [`Renderer`].
//!
//! The only file in the project that knows what a terminal is. Everything above
//! it draws into a [`Frame`] and never learns where that frame went, which is
//! what makes swapping this for a hardware screen a change to one file.

use std::io::{self, Stdout, Write};
use std::mem::MaybeUninit;

use crate::device::DeviceProfile;
use crate::ui::{Cell, Frame, RenderError, Renderer};

const ENTER_ALTERNATE_SCREEN: &str = "\u{1b}[?1049h";
const LEAVE_ALTERNATE_SCREEN: &str = "\u{1b}[?1049l";
const HIDE_CURSOR: &str = "\u{1b}[?25l";
const SHOW_CURSOR: &str = "\u{1b}[?25h";

/// A [`Renderer`] that writes a frame as escape sequences to anything taking
/// bytes.
///
/// Only cells that differ from the last frame are written, and cells that
/// changed next to each other on a row go out as one run after a single cursor
/// move. Writing the whole screen every frame is what makes a terminal UI feel
/// slow, and on the target device the screen is the slowest thing in the loop.
///
/// The first frame has nothing to compare against, so it is written in full. So
/// is the frame after a failed write, because a screen that rejected part of a
/// frame is no longer known to match anything.
pub struct FrameWriter<W: Write> {
    sink: W,
    previous: Option<Frame>,
}

impl<W: Write> FrameWriter<W> {
    /// A writer whose first frame will be written in full.
    pub fn new(sink: W) -> Self {
        Self {
            sink,
            previous: None,
        }
    }

    /// What frames are being written to.
    pub fn sink(&self) -> &W {
        &self.sink
    }
}

fn differs(previous: Option<&Frame>, frame: &Frame, column: usize, row: usize) -> bool {
    match previous {
        None => true,
        Some(previous) => previous.get(column, row) != frame.get(column, row),
    }
}

impl<W: Write> Renderer for FrameWriter<W> {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        let screen = DeviceProfile::TARGET.screen;
        let previous = self.previous.take();

        for row in 0..screen.rows {
            let mut column = 0;
            while column < screen.columns {
                if !differs(previous.as_ref(), frame, column, row) {
                    column += 1;
                    continue;
                }

                let run_starts_at = column;
                let mut run = String::new();
                while column < screen.columns && differs(previous.as_ref(), frame, column, row) {
                    run.push(frame.get(column, row).unwrap_or(Cell::BLANK).glyph());
                    column += 1;
                }

                write!(self.sink, "\u{1b}[{};{}H{run}", row + 1, run_starts_at + 1)
                    .map_err(|_| RenderError::WriteFailed)?;
            }
        }

        self.sink.flush().map_err(|_| RenderError::WriteFailed)?;
        self.previous = Some(frame.clone());
        Ok(())
    }
}

fn current_mode() -> Result<libc::termios, RenderError> {
    let mut mode = MaybeUninit::<libc::termios>::uninit();

    /* SAFETY: tcgetattr either fills the termios behind the pointer and
    returns zero, or returns non-zero having written nothing. The pointer is
    to local storage that outlives the call, and the value below is only
    read once the call has reported success. */
    let outcome = unsafe { libc::tcgetattr(libc::STDIN_FILENO, mode.as_mut_ptr()) };
    if outcome != 0 {
        return Err(RenderError::Unavailable);
    }

    /* SAFETY: tcgetattr reported success, so it initialised every field. */
    Ok(unsafe { mode.assume_init() })
}

fn raw_from(mode: libc::termios) -> libc::termios {
    let mut raw = mode;

    /* SAFETY: cfmakeraw only writes through the pointer it is given, which is
    a local copy owned by this function and valid for the call. */
    unsafe { libc::cfmakeraw(&raw mut raw) };

    raw
}

fn apply_mode(mode: &libc::termios) -> Result<(), RenderError> {
    /* SAFETY: tcsetattr reads the termios through the pointer and does not
    keep it. The reference guarantees it is valid for the call. */
    let outcome = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, mode) };
    if outcome != 0 {
        return Err(RenderError::Unavailable);
    }
    Ok(())
}

fn begin_drawing() -> Result<(), RenderError> {
    let mut out = io::stdout();
    write!(out, "{ENTER_ALTERNATE_SCREEN}{HIDE_CURSOR}").map_err(|_| RenderError::WriteFailed)?;
    out.flush().map_err(|_| RenderError::WriteFailed)
}

/// The screen a terminal presents, given back as it was found.
///
/// Opening one switches the terminal into raw mode and onto its alternate
/// screen; dropping it puts both back. Drop runs when a caller returns early
/// with `?` and while a panic unwinds, which is what makes the restore hold on
/// the paths that are easiest to forget.
pub struct TerminalScreen {
    writer: FrameWriter<Stdout>,
    entry_mode: libc::termios,
}

impl TerminalScreen {
    /// Take the terminal over for drawing.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Unavailable`] when standard input is not a
    /// terminal, and [`RenderError::WriteFailed`] when the terminal will not
    /// switch to its alternate screen. Neither leaves the terminal altered.
    pub fn open() -> Result<Self, RenderError> {
        let entry_mode = current_mode()?;
        apply_mode(&raw_from(entry_mode))?;

        if let Err(failed) = begin_drawing() {
            let _ = apply_mode(&entry_mode);
            return Err(failed);
        }

        Ok(Self {
            writer: FrameWriter::new(io::stdout()),
            entry_mode,
        })
    }
}

impl Renderer for TerminalScreen {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        self.writer.render(frame)
    }
}

impl Drop for TerminalScreen {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = write!(out, "{SHOW_CURSOR}{LEAVE_ALTERNATE_SCREEN}");
        let _ = out.flush();
        let _ = apply_mode(&self.entry_mode);
    }
}
