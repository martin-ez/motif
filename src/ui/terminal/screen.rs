//! Taking a terminal over for drawing and typing, and giving it back.
//!
//! A shell over `libc`: switching modes is three calls into the C library and
//! no decision of our own. The behaviour that is ours — which cells to write,
//! where the screen's edges fall, and which control a key stands for — is in
//! [`FrameWriter`](super::FrameWriter), [`Viewport`](super::Viewport) and
//! [`KeyReader`](super::KeyReader), where a test can reach it.

use std::io::{self, Stdin, Stdout, Write};
use std::mem::MaybeUninit;

use super::{KeyReader, Viewport};
use crate::device::Control;
use crate::ui::{ControlEvent, Controls, Frame, Hint, Panel, RenderError, Renderer};

const ENTER_ALTERNATE_SCREEN: &str = "\u{1b}[?1049h";
const LEAVE_ALTERNATE_SCREEN: &str = "\u{1b}[?1049l";
const HIDE_CURSOR: &str = "\u{1b}[?25l";
const SHOW_CURSOR: &str = "\u{1b}[?25h";
const WIPE_SCREEN: &str = "\u{1b}[2J";

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

fn polling_raw_from(mode: libc::termios) -> libc::termios {
    let mut raw = mode;

    /* SAFETY: cfmakeraw only writes through the pointer it is given, which is
    a local copy owned by this function and valid for the call. */
    unsafe { libc::cfmakeraw(&raw mut raw) };

    raw.c_cc[libc::VMIN] = 0;
    raw.c_cc[libc::VTIME] = 0;

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
    write!(out, "{ENTER_ALTERNATE_SCREEN}{HIDE_CURSOR}{WIPE_SCREEN}")
        .map_err(|_| RenderError::WriteFailed)?;
    out.flush().map_err(|_| RenderError::WriteFailed)
}

fn window_size() -> Option<(usize, usize)> {
    let mut size = MaybeUninit::<libc::winsize>::uninit();

    /* SAFETY: TIOCGWINSZ either fills the winsize behind the pointer and
    returns zero, or returns non-zero having written nothing. The pointer is to
    local storage that outlives the call, and the value below is only read once
    the call has reported success. */
    let outcome = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) };
    if outcome != 0 {
        return None;
    }

    /* SAFETY: the ioctl reported success, so it initialised every field. */
    let size = unsafe { size.assume_init() };

    Some((size.ws_col as usize, size.ws_row as usize))
}

fn centred_origin() -> (usize, usize) {
    let Some((columns, rows)) = window_size() else {
        return (0, 0);
    };

    (
        columns.saturating_sub(Viewport::<Stdout>::COLUMNS) / 2,
        rows.saturating_sub(Viewport::<Stdout>::ROWS) / 2,
    )
}

/// The terminal a player has, given back as it was found.
///
/// The screen and the panel are one object because the mode they need is one
/// setting: raw mode stops the terminal echoing keys and buffering them until a
/// newline, and it belongs to the terminal rather than to either half.
///
/// Opening one switches the terminal into raw mode and onto its alternate
/// screen; dropping it puts both back, including while a panic unwinds.
///
/// Frames go out through a [`Viewport`], so a player sees the panel's screen
/// with its edges drawn rather than a frame in the corner of a window.
pub struct TerminalScreen {
    writer: Viewport<Stdout>,
    reader: KeyReader<Stdin>,
    entry_mode: libc::termios,
}

impl TerminalScreen {
    /// Take the terminal over for drawing and typing.
    ///
    /// Keys are read without waiting: the raw mode applied here hands back
    /// whatever has been typed and returns immediately, so a frame is never
    /// spent waiting for the player.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Unavailable`] when standard input is not a
    /// terminal, and [`RenderError::WriteFailed`] when the terminal will not
    /// switch to its alternate screen. Neither leaves the terminal altered.
    pub fn open() -> Result<Self, RenderError> {
        let entry_mode = current_mode()?;
        apply_mode(&polling_raw_from(entry_mode))?;

        if let Err(failed) = begin_drawing() {
            let _ = apply_mode(&entry_mode);
            return Err(failed);
        }

        let origin = centred_origin();

        Ok(Self {
            writer: Viewport::at(io::stdout(), origin.0, origin.1),
            reader: KeyReader::new(io::stdin()),
            entry_mode,
        })
    }

    /// The panel and the screen, borrowed apart.
    ///
    /// A terminal is one object that is both halves, and an event loop holds
    /// them as two: taking controls and rendering are separate on hardware,
    /// where the keys and the screen are separate devices. Splitting is what
    /// lets the terminal be both without the loop having to assume they always
    /// arrive together.
    pub fn split(&mut self) -> (&mut KeyReader<Stdin>, CentredScreen<'_>) {
        (
            &mut self.reader,
            CentredScreen {
                viewport: &mut self.writer,
            },
        )
    }
}

/// The screen half of a [`TerminalScreen`], kept in the middle of the window.
///
/// Where the box sits is measured again every frame, so dragging the window to
/// another size puts the panel back in the middle of it rather than leaving it
/// where the terminal used to end. The measurement is one `ioctl` against a
/// frame budget of tens of milliseconds, and a frame whose window has not moved
/// writes nothing extra at all.
///
/// A window too small to hold the box puts it at the top left instead, where as
/// much of it as fits can be seen.
pub struct CentredScreen<'a> {
    viewport: &'a mut Viewport<Stdout>,
}

impl Renderer for CentredScreen<'_> {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        let (column, row) = centred_origin();
        self.viewport.place(column, row);
        self.viewport.render(frame)
    }

    fn show_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        self.viewport.show_panel(panel)
    }
}

impl Controls for TerminalScreen {
    fn hint(&self, control: Control) -> Option<Hint> {
        self.reader.hint(control)
    }

    fn poll(&mut self) -> Option<ControlEvent> {
        self.reader.poll()
    }

    fn interrupted(&self) -> bool {
        self.reader.interrupted()
    }
}

impl Renderer for TerminalScreen {
    fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        self.writer.render(frame)
    }

    fn show_panel(&mut self, panel: &Panel) -> Result<(), RenderError> {
        self.writer.show_panel(panel)
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
