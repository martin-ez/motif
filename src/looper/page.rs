//! The looper screen: the panel's transport buttons, and what the loop is doing.
//!
//! The page holds the transport because the player drives it, and reads the
//! playhead because the callback drives that. Splitting them that way is what
//! keeps one copy of each: a page that kept its own idea of where the loop was
//! would drift from the thread actually moving it, and a callback that kept its
//! own transport would drift from the buttons.
//!
//! The input gain and its mute are held here for the first of those reasons,
//! and go down the same queue the transport does: the page owns what the
//! player asked for, and the engine is told rather than read.
//!
//! Nothing here names a key, a terminal or an escape sequence. The page is
//! handed [`ControlEvent`]s and fills a [`Region`], so the same page draws on a
//! hardware panel once there is one.

use crate::audio::{Command, CommandSender, Commanded, Gain, SampleClockReader, command_channel};
use crate::device::{AudioProfile, Button, DeviceProfile, Encoder};
use crate::looper::{
    LoopBuffer, LoopEngine, LoopMarks, MarksReader, PositionReader, TakeReader, Transport,
    WaveformReader, position_meter, take_handoff, waveform_meter,
};
use crate::seq::{BeatGrid, TapTempo};
use crate::ui::bar::{BRACKETS, FILLED, UNFILLED, bracketed};
use crate::ui::{ControlEvent, FLOOR_DBFS, Page, Region, Turn, amplitude, decibels};

const QUEUED_COMMANDS: usize = 8;
const STATE_ROW: usize = 0;
const ARMED_COLUMN: usize = 14;
const TEMPO_ROW: usize = 1;
const READOUT_ROW: usize = 2;
const BAR_ROW: usize = 3;
const MARKS_ROW: usize = 4;
const WAVEFORM_ROW: usize = MARKS_ROW + LoopMarks::ROWS;
const WAVEFORM_ROWS: usize = 4;
const GAIN_ROW: usize = WAVEFORM_ROW + WAVEFORM_ROWS;
const STACK_ROW: usize = GAIN_ROW + 1;
const MUTE_COLUMN: usize = 12;
const ARMED: &str = "ARMED";
const MUTED: &str = "MUTE";
const DECIBELS_PER_DETENT: f32 = 1.0;
const UNITY_DECIBELS: f32 = 0.0;
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

fn ceiling_decibels() -> f32 {
    decibels(Gain::CEILING)
}

fn bar(playhead: u32, recorded: u32, columns: usize) -> String {
    let width = columns.saturating_sub(BRACKETS);
    let filled = match recorded {
        0 => 0,
        recorded => width * playhead as usize / recorded as usize,
    };

    bracketed(width, |cell| if cell < filled { FILLED } else { UNFILLED })
}

/// The screen a player operates the looper from.
///
/// Record opens the first take, records again to layer onto it, and drops back
/// out of the layer; play closes whatever is open and runs the loop; stop halts
/// it keeping what was recorded. Shift makes a second gesture of each: play
/// taps a pulse, record mutes the input, stop takes the last layer off, and
/// shift with down empties the loop. The encoder moves the input gain a decibel
/// a detent, and every other control is left alone, so the page can sit under a
/// shell that uses them for something else.
///
/// ```
/// use motif::audio::{command_channel, sample_clock};
/// use motif::device::Button;
/// use motif::looper::{LooperPage, Transport, marks_handoff, position_meter, waveform_meter};
/// use motif::ui::{ControlEvent, Page};
///
/// let (_writer, reader) = position_meter();
/// let shape = waveform_meter().1;
/// let marks = marks_handoff().1;
/// let clock = sample_clock(48_000).1;
/// let mut page = LooperPage::new(reader, shape, marks, clock, command_channel(8).0);
///
/// page.control(ControlEvent::Pressed { button: Button::Record, shifted: false });
///
/// assert_eq!(page.transport(), Transport::Recording);
/// ```
pub struct LooperPage {
    transport: Transport,
    ordered_transport: Transport,
    ordered_decibels: f32,
    ordered_muted: bool,
    commands: CommandSender,
    position: PositionReader,
    waveform: WaveformReader,
    analysis: MarksReader,
    marks: LoopMarks,
    elapsed: SampleClockReader,
    taps: TapTempo,
    decibels: f32,
    muted: bool,
    undos: usize,
    emptying: bool,
}

impl LooperPage {
    /// A page over an idle transport, reading its playhead from `position`, the
    /// shape of the loop from `waveform` and what analysis found from `marks`,
    /// timing its taps by `elapsed`, and ordering the engine over `commands`.
    ///
    /// A tap is stamped with the frame the device had reached, so the grid it
    /// makes lines up with the audio captured around it.
    pub fn new(
        position: PositionReader,
        waveform: WaveformReader,
        marks: MarksReader,
        elapsed: SampleClockReader,
        commands: CommandSender,
    ) -> Self {
        Self {
            transport: Transport::default(),
            ordered_transport: Transport::default(),
            ordered_decibels: UNITY_DECIBELS,
            ordered_muted: false,
            taps: TapTempo::new(elapsed.sample_rate()),
            commands,
            position,
            waveform,
            analysis: marks,
            marks: LoopMarks::none(),
            elapsed,
            decibels: UNITY_DECIBELS,
            muted: false,
            undos: 0,
            emptying: false,
        }
    }

    /// A page, the engine it drives, and the finished takes it hands over.
    ///
    /// The page holds the reading end of the playhead and of the loop's shape,
    /// the end of `marks` an analyst publishes to, and the sending end of the
    /// command queue; the engine holds the other end of each and the loop
    /// itself, sized from `profile`. Taps are timed by `elapsed`, and the third
    /// end returned is where a finished take crosses to whatever analyses it.
    ///
    /// All of it is allocated here and never again, so this belongs in setup.
    /// The engine is what a stream plays, so it goes to whatever opens one.
    pub fn driving(
        profile: AudioProfile,
        marks: MarksReader,
        elapsed: SampleClockReader,
    ) -> (Self, Commanded<LoopEngine>, TakeReader) {
        let (commands, orders) = command_channel(QUEUED_COMMANDS);
        let (publishing, playhead) = position_meter();
        let (drawing, shape) = waveform_meter();
        let (crossing, takes) = take_handoff(profile);

        (
            Self::new(playhead, shape, marks, elapsed, commands),
            Commanded::new(
                orders,
                LoopEngine::new(profile, publishing, drawing, crossing),
            ),
            takes,
        )
    }

    fn retime_taps_to_the_clock(&mut self) {
        if self.taps.grid().sample_rate() != self.elapsed.sample_rate() {
            self.taps = TapTempo::new(self.elapsed.sample_rate());
        }
    }

    fn order_transport(&mut self) {
        if self.ordered_transport == self.transport {
            return;
        }

        if self
            .commands
            .send(Command::SetTransport(self.transport))
            .is_ok()
        {
            self.ordered_transport = self.transport;
        }
    }

    fn order_gain(&mut self) {
        if self.ordered_decibels == self.decibels {
            return;
        }

        if self.commands.send(Command::SetGain(self.gain())).is_ok() {
            self.ordered_decibels = self.decibels;
        }
    }

    fn order_mute(&mut self) {
        if self.ordered_muted == self.muted {
            return;
        }

        if self.commands.send(Command::SetMuted(self.muted)).is_ok() {
            self.ordered_muted = self.muted;
        }
    }

    fn order_undos(&mut self) {
        while self.undos > 0 && self.commands.send(Command::Undo).is_ok() {
            self.undos -= 1;
        }
    }

    fn order_emptying(&mut self) {
        if self.emptying && self.commands.send(Command::Clear).is_ok() {
            self.emptying = false;
        }
    }

    fn order_the_engine(&mut self) {
        self.order_transport();
        self.order_gain();
        self.order_mute();
        self.order_undos();
        self.order_emptying();
    }

    fn undo_a_layer(&mut self) {
        self.undos += 1;
        if self.transport == Transport::Overdubbing {
            self.transport = Transport::Playing;
        }
        self.order_the_engine();
    }

    fn empty_the_loop(&mut self) {
        self.emptying = true;
        self.transport = Transport::Idle;
        self.order_the_engine();
    }

    /// What the looper is doing.
    ///
    /// What the page ordered rather than what the engine has reached: the order
    /// crosses a queue, so the two agree a block later.
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

    /// Show `marks` over the loop, replacing whatever analysis found last.
    ///
    /// What [`draw`](Page::draw) does with every pass an analyst publishes, and
    /// what a caller with an analysis of its own can do directly.
    ///
    /// They are drawn against the summary the page is already reading, so a
    /// mark lands on the column of the loop it was found in; one found in a
    /// take the player has since recorded over is dropped rather than drawn
    /// somewhere it does not belong.
    pub fn analysed(&mut self, marks: LoopMarks) {
        self.marks = marks;
    }

    /// Where the input gain sits, in decibels, with zero at unity.
    ///
    /// Decibels because that is the scale the control moves in and the screen
    /// shows: a detent of the encoder is a decibel wherever the gain already
    /// is, where a linear step would be a leap at the bottom of the range and
    /// imperceptible at the top.
    pub const fn decibels(&self) -> f32 {
        self.decibels
    }

    /// The input gain as a linear multiplier, with `1.0` at unity.
    ///
    /// What the page orders as
    /// [`Command::SetGain`](crate::audio::Command::SetGain), which carries a
    /// multiplier rather than a scale reading.
    pub fn gain(&self) -> f32 {
        amplitude(self.decibels)
    }

    /// Whether the player has muted the input.
    ///
    /// Ordered as [`Command::SetMuted`](crate::audio::Command::SetMuted), and
    /// kept apart from the gain so that unmuting returns to the level that was
    /// set rather than to unity.
    pub const fn muted(&self) -> bool {
        self.muted
    }

    fn nudge_the_gain(&mut self, decibels: f32) {
        self.decibels = (self.decibels + decibels).clamp(FLOOR_DBFS, ceiling_decibels());
    }
}

impl Page for LooperPage {
    fn control(&mut self, event: ControlEvent) {
        if let ControlEvent::Pressed {
            button: Button::Play,
            shifted: true,
        } = event
        {
            self.retime_taps_to_the_clock();
            let _joined = self.taps.tap(self.elapsed.read());
            return;
        }

        if let ControlEvent::Pressed {
            button: Button::Record,
            shifted: true,
        } = event
        {
            self.muted = !self.muted;
            self.order_mute();
            return;
        }

        if let ControlEvent::Pressed {
            button: Button::Stop,
            shifted: true,
        } = event
        {
            self.undo_a_layer();
            return;
        }

        if let ControlEvent::Pressed {
            button: Button::Down,
            shifted: true,
        } = event
        {
            self.empty_the_loop();
            return;
        }

        if let ControlEvent::Turned {
            encoder: Encoder::Main,
            turn,
            ..
        } = event
        {
            self.nudge_the_gain(match turn {
                Turn::Clockwise => DECIBELS_PER_DETENT,
                Turn::Anticlockwise => -DECIBELS_PER_DETENT,
            });
            self.order_gain();
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
            self.order_transport();
        }
    }

    /// Every control is ordered again here where the queue was full when it was
    /// moved, an order the engine never took leaving the two disagreeing for as
    /// long as the run lasts.
    fn draw(&mut self, mut region: Region<'_>) {
        self.order_the_engine();
        if let Some(found) = self.analysis.read() {
            self.analysed(found);
        }
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

        let waveform = self.waveform.read();
        for (offset, drawn) in self
            .marks
            .drawn(&waveform, region.columns())
            .iter()
            .enumerate()
        {
            region.write(0, MARKS_ROW + offset, drawn);
        }

        let shape = waveform.drawn(region.columns(), WAVEFORM_ROWS);
        for (offset, drawn) in shape.iter().enumerate() {
            region.write(0, WAVEFORM_ROW + offset, drawn);
        }

        region.write(0, GAIN_ROW, &format!("IN {:>5.1} dB", self.decibels));
        if self.muted {
            region.write(MUTE_COLUMN, GAIN_ROW, MUTED);
        }

        region.write(
            0,
            STACK_ROW,
            &format!("LAYERS {}/{}", position.depth(), LoopBuffer::LAYERS),
        );
    }
}
