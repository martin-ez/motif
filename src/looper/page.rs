//! The looper screen: the panel's transport buttons, and what the loop is doing.
//!
//! The page holds the transport because the player drives it, and reads the
//! playhead because the callback drives that. Splitting them that way is what
//! keeps one copy of each: a page that kept its own idea of where the loop was
//! would drift from the thread actually moving it, and a callback that kept its
//! own transport would drift from the buttons.
//!
//! Nothing here names a key, a terminal or an escape sequence. The page is
//! handed [`ControlEvent`]s and fills a [`Frame`], so the same page draws on a
//! hardware panel once there is one.

use crate::device::{Button, DeviceProfile};
use crate::looper::{PositionReader, Transport};
use crate::ui::{App, Cell, ControlEvent, Flow, Frame, Legend};

const STATE_ROW: usize = 0;
const ARMED_COLUMN: usize = 14;
const READOUT_ROW: usize = 2;
const BAR_ROW: usize = 3;
const ARMED: &str = "ARMED";
const FILLED: char = '#';
const UNFILLED: char = '-';
const TENTHS_PER_SECOND: u64 = 10;
const SECONDS_PER_MINUTE: u64 = 60;

fn named(transport: Transport) -> &'static str {
    match transport {
        Transport::Idle => "IDLE",
        Transport::Recording => "RECORDING",
        Transport::Playing => "PLAYING",
        Transport::Overdubbing => "OVERDUBBING",
        Transport::Stopped => "STOPPED",
    }
}

fn write(frame: &mut Frame, column: usize, row: usize, text: &str) {
    for (offset, glyph) in text.chars().enumerate() {
        frame.set(column + offset, row, Cell::new(glyph));
    }
}

fn clock(frames: u32) -> String {
    let tenths =
        u64::from(frames) * TENTHS_PER_SECOND / u64::from(DeviceProfile::TARGET.audio.sample_rate);
    let seconds = tenths / TENTHS_PER_SECOND;

    format!(
        "{}:{:02}.{}",
        seconds / SECONDS_PER_MINUTE,
        seconds % SECONDS_PER_MINUTE,
        tenths % TENTHS_PER_SECOND
    )
}

fn bar(playhead: u32, recorded: u32) -> String {
    let width = DeviceProfile::TARGET.screen.columns.saturating_sub(2);
    let filled = match recorded {
        0 => 0,
        recorded => width * playhead as usize / recorded as usize,
    };

    let mut drawn = String::with_capacity(width + 2);
    drawn.push('[');
    drawn.extend(std::iter::repeat_n(FILLED, filled));
    drawn.extend(std::iter::repeat_n(UNFILLED, width - filled));
    drawn.push(']');

    drawn
}

/// The screen a player operates the looper from.
///
/// Record opens the first take, records again to layer onto it, and drops back
/// out of the layer; play closes whatever is open and runs the loop; stop halts
/// it keeping what was recorded. Every other control is left alone, so the page
/// can sit under a shell that uses them for something else.
///
/// The page never ends the run. Quitting is the shell's to decide, not a
/// screen's.
///
/// ```
/// use motif::device::Button;
/// use motif::looper::{LooperPage, Transport, position_meter};
/// use motif::ui::{App, ControlEvent};
///
/// let (_writer, reader) = position_meter();
/// let mut page = LooperPage::new(reader);
///
/// page.control(ControlEvent::Pressed { button: Button::Record, shifted: false });
///
/// assert_eq!(page.transport(), Transport::Recording);
/// ```
pub struct LooperPage {
    transport: Transport,
    position: PositionReader,
}

impl LooperPage {
    /// A page over an idle transport, reading its playhead from `position`.
    pub fn new(position: PositionReader) -> Self {
        Self {
            transport: Transport::default(),
            position,
        }
    }

    /// What the looper is doing.
    ///
    /// Public because the transport is what the engine has to be told: a
    /// composition holding this page and a command queue forwards
    /// [`Command::SetArmed`](crate::audio::Command::SetArmed) from
    /// [`Transport::captures_input`] rather than tracking the presses a second
    /// time.
    pub const fn transport(&self) -> Transport {
        self.transport
    }
}

impl App for LooperPage {
    fn control(&mut self, event: ControlEvent) -> Flow {
        if let ControlEvent::Pressed { button, .. } = event {
            self.transport = match button {
                Button::Record => self.transport.record(),
                Button::Play => self.transport.play(),
                Button::Stop => self.transport.stop(),
                Button::Up
                | Button::Down
                | Button::Left
                | Button::Right
                | Button::FirstScene
                | Button::SecondScene
                | Button::ThirdScene
                | Button::FourthScene
                | Button::Shift => self.transport,
            };
        }

        Flow::Continue
    }

    fn legend(&self) -> Legend {
        Legend::blank()
            .answering(Button::Play)
            .answering(Button::Stop)
            .answering(Button::Record)
    }

    fn draw(&mut self, frame: &mut Frame) -> Flow {
        let position = self.position.read();

        write(frame, 0, STATE_ROW, named(self.transport));
        if self.transport.captures_input() {
            write(frame, ARMED_COLUMN, STATE_ROW, ARMED);
        }

        let readout = format!(
            "{} / {}",
            clock(position.playhead()),
            clock(position.recorded())
        );
        write(frame, 0, READOUT_ROW, &readout);
        write(
            frame,
            0,
            BAR_ROW,
            &bar(position.playhead(), position.recorded()),
        );

        Flow::Continue
    }
}
