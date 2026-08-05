//! The looper screen: the panel's transport buttons, and what the loop is doing.
//!
//! The page holds the transport because the player drives it, and reads the
//! playhead because the callback drives that. Splitting them that way is what
//! keeps one copy of each: a page that kept its own idea of where the loop was
//! would drift from the thread actually moving it, and a callback that kept its
//! own transport would drift from the buttons.
//!
//! Nothing here names a key, a terminal or an escape sequence. The page is
//! handed [`ControlEvent`]s and fills a [`Region`], so the same page draws on a
//! hardware panel once there is one.

use crate::audio::SampleClockReader;
use crate::device::{Button, DeviceProfile};
use crate::looper::{PositionReader, Transport};
use crate::seq::{BeatGrid, TapTempo};
use crate::ui::{ControlEvent, Legend, Page, Region};

const STATE_ROW: usize = 0;
const ARMED_COLUMN: usize = 14;
const TEMPO_ROW: usize = 1;
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

fn bar(playhead: u32, recorded: u32, columns: usize) -> String {
    let width = columns.saturating_sub(2);
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
/// it keeping what was recorded. Held with shift, play taps a pulse instead of
/// starting one. Every other control is left alone, so the page can sit under a
/// shell that uses them for something else.
///
/// ```
/// use motif::audio::sample_clock;
/// use motif::device::Button;
/// use motif::looper::{LooperPage, Transport, position_meter};
/// use motif::ui::{ControlEvent, Page};
///
/// let (_writer, reader) = position_meter();
/// let mut page = LooperPage::new(reader, sample_clock(48_000).1);
///
/// page.control(ControlEvent::Pressed { button: Button::Record, shifted: false });
///
/// assert_eq!(page.transport(), Transport::Recording);
/// ```
pub struct LooperPage {
    transport: Transport,
    position: PositionReader,
    elapsed: SampleClockReader,
    taps: TapTempo,
}

impl LooperPage {
    /// A page over an idle transport, reading its playhead from `position` and
    /// timing its taps by `elapsed`.
    ///
    /// A tap is stamped with the frame the device had reached, so the grid it
    /// makes lines up with the audio captured around it.
    pub fn new(position: PositionReader, elapsed: SampleClockReader) -> Self {
        Self {
            transport: Transport::default(),
            taps: TapTempo::new(elapsed.sample_rate()),
            position,
            elapsed,
        }
    }

    /// What the looper is doing.
    ///
    /// Public because the transport is what the engine has to be told: a
    /// composition holding this page and a command queue forwards this state
    /// as [`Command::SetTransport`](crate::audio::Command::SetTransport) rather
    /// than tracking the presses a second time.
    pub const fn transport(&self) -> Transport {
        self.transport
    }

    /// The beats the player has tapped.
    ///
    /// Public for the reason [`transport`](Self::transport) is: the grid is
    /// what the engine has to be given, and the taps are the grid rather than a
    /// tempo worked out from them.
    pub const fn grid(&self) -> &BeatGrid {
        self.taps.grid()
    }
}

impl Page for LooperPage {
    fn control(&mut self, event: ControlEvent) {
        if let ControlEvent::Pressed {
            button: Button::Play,
            shifted: true,
        } = event
        {
            let _joined = self.taps.tap(self.elapsed.read());
            return;
        }

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
    }

    fn legend(&self) -> Legend {
        Legend::blank()
            .answering(Button::Play)
            .answering(Button::Stop)
            .answering(Button::Record)
    }

    fn draw(&mut self, mut region: Region<'_>) {
        let position = self.position.read();

        region.write(0, STATE_ROW, named(self.transport));
        if self.transport.captures_input() {
            region.write(ARMED_COLUMN, STATE_ROW, ARMED);
        }
        if let Some(tempo) = self.taps.tempo() {
            region.write(0, TEMPO_ROW, &format!("{tempo:.1} BPM"));
        }

        let readout = format!(
            "{} / {}",
            clock(position.playhead()),
            clock(position.recorded())
        );
        region.write(0, READOUT_ROW, &readout);
        region.write(
            0,
            BAR_ROW,
            &bar(position.playhead(), position.recorded(), region.columns()),
        );
    }
}
