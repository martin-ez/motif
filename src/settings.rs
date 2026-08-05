//! Configuring the application from a page, starting with the audio path.
//!
//! [`AudioPage`] holds the [`DeviceLink`] it is configuring rather than
//! reporting a choice for something else to apply, because applying one is
//! opening a stream: the listing, the choice and the device that has to answer
//! for it are one mechanism, and splitting them puts a stale selection between
//! the two halves.
//!
//! The listing comes from a [`DeviceCatalog`] refreshed before the first stream
//! opens, which is what keeps enumeration off a running device. A listing was
//! true when it was drawn and nothing promises it still is, so a choice the
//! backend refuses rolls back to the one that was running and the reason is
//! drawn.

use crate::audio::{
    AudioBackend, AudioDevice, AudioHost, AudioPath, AudioState, ChannelSelection, DeviceCatalog,
    DeviceError, DeviceId, DeviceLink, DeviceSelection,
};
use crate::closed_set::closed_set;
use crate::device::{Button, Encoder};
use crate::ui::{Cell, ControlEvent, Frame, Legend, Page, Turn};

closed_set! {
    /// One thing about the audio path a player chooses.
    ///
    /// A closed set rather than a row count, for the reason the panel's
    /// controls are one: a `match` over the set stops compiling when the page
    /// gains a setting, and a setting's position here is the row it is drawn
    /// on.
    enum AudioSetting;
    /// Every setting, in the order the page draws them.
    ///
    /// Order runs from the widest choice to the narrowest, because choosing a
    /// host replaces the devices under it and choosing a device replaces the
    /// channels under that.
    const ALL;
    /// The audio API both devices are reached through.
    Host,
    /// The device audio is captured from.
    Input,
    /// Which of the input device's channels are captured.
    InputChannels,
    /// The device audio is played to.
    Output,
    /// Which of the output device's channels are played to.
    OutputChannels,
}

const MARKER: char = '>';
const MARKER_COLUMN: usize = 0;
const LABEL_COLUMN: usize = 2;
const VALUE_COLUMN: usize = 19;
const REFUSED_ROW: usize = AudioSetting::ALL.len() + 1;
const REFUSED: &str = "cannot open: ";
const NO_CHANNELS: &str = "none";

fn label(setting: AudioSetting) -> &'static str {
    match setting {
        AudioSetting::Host => "host",
        AudioSetting::Input => "input",
        AudioSetting::InputChannels => "input channels",
        AudioSetting::Output => "output",
        AudioSetting::OutputChannels => "output channels",
    }
}

fn counted(channels: ChannelSelection) -> String {
    let first = u32::from(channels.first) + 1;

    match channels.count {
        0 => NO_CHANNELS.to_owned(),
        1 => first.to_string(),
        count => format!("{first}-{}", first + u32::from(count) - 1),
    }
}

fn widest(device: &AudioDevice) -> u16 {
    device.channels.last().copied().unwrap_or(0)
}

fn runs(width: u16) -> Vec<ChannelSelection> {
    (1..=width)
        .flat_map(|count| (0..=width - count).map(move |first| ChannelSelection { first, count }))
        .collect()
}

fn stepped(at: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;

    Some(match at {
        None => 0,
        Some(at) if forward => (at + 1).min(last),
        Some(at) => at.saturating_sub(1),
    })
}

fn found(devices: &[AudioDevice], id: &DeviceId) -> Option<usize> {
    devices.iter().position(|device| device.id == *id)
}

fn taken_whole(device: &AudioDevice) -> ChannelSelection {
    ChannelSelection::all(widest(device))
}

/// The page a player chooses the audio path from.
///
/// Five settings drawn as rows: the arrows move between them, and the arrows
/// across or the encoder move the selected row's value. A value that moves is
/// applied at once, and the row shows what the link is open on rather than
/// what was asked for.
///
/// A choice the backend refuses leaves the previous one running and draws why,
/// an interface unplugged since the list was drawn being ordinary rather than
/// exceptional.
///
/// ```
/// use motif::audio::{
///     AudioBackend, DeviceLink, NullBackend, Passthrough, StreamConfig, StreamRequest,
/// };
/// use motif::settings::{AudioPage, AudioSetting};
///
/// let granted = StreamConfig {
///     sample_rate: 48_000,
///     block_size: 256,
///     input_channels: 2,
///     output_channels: 2,
/// };
/// let request = StreamRequest { sample_rate: 48_000, block_size: 256 };
/// let backend = NullBackend::rounding(granted);
/// let selection = backend.defaults(48_000).expect("the null backend has both directions");
///
/// let page = AudioPage::opened(DeviceLink::new(backend, request, selection, Passthrough::new));
///
/// assert_eq!(page.selected(), AudioSetting::Host);
/// assert_eq!(page.value(AudioSetting::InputChannels), "1-2");
/// ```
pub struct AudioPage<B: AudioBackend, F> {
    link: DeviceLink<B, F>,
    catalog: DeviceCatalog,
    row: usize,
    refused: Option<DeviceError>,
}

impl<B: AudioBackend, F, P> AudioPage<B, F>
where
    F: FnMut() -> P,
    P: AudioPath,
{
    /// List what `link` could be opened on, then open and start it.
    ///
    /// Enumerating before the first stream is what keeps a running device out
    /// of the listing's way: on ALSA, listing a device a stream holds takes
    /// `EBUSY` and drops the one row the page must not lose.
    ///
    /// This cannot fail. A link whose device refuses is left in
    /// [`AudioState::Lost`] carrying why, that being something to draw rather
    /// than a reason not to run.
    pub fn opened(link: DeviceLink<B, F>) -> Self {
        let mut page = Self {
            catalog: DeviceCatalog::new(link.request().sample_rate),
            link,
            row: 0,
            refused: None,
        };

        page.catalog.refresh(page.link.backend(), None);
        if page.link.open().is_ok() {
            let _started = page.link.start();
        }

        page
    }

    /// Enumerate again, keeping whatever the link is open on.
    ///
    /// The listing is what the rows are chosen from, and it ages: a device
    /// plugged in after the page opened is reachable only once this has been
    /// called. Blocks and allocates; never reach it from the audio callback.
    pub fn refresh(&mut self) {
        self.catalog
            .refresh(self.link.backend(), Some(self.link.selection()));
    }

    /// Which setting the player is on.
    pub fn selected(&self) -> AudioSetting {
        AudioSetting::ALL[self.row]
    }

    /// What the page draws for `setting`, taken from what the link is open on.
    pub fn value(&self, setting: AudioSetting) -> String {
        let selection = self.link.selection();

        match setting {
            AudioSetting::Host => selection.host.clone(),
            AudioSetting::Input => selection.input.to_string(),
            AudioSetting::InputChannels => counted(selection.input_channels),
            AudioSetting::Output => selection.output.to_string(),
            AudioSetting::OutputChannels => counted(selection.output_channels),
        }
    }

    /// The hosts and devices the rows are chosen from, as of the last listing.
    pub fn listed(&self) -> &[AudioHost] {
        self.catalog.hosts()
    }

    /// The link the page is configuring.
    ///
    /// This is the route to what the stream knows and the page does not: the
    /// selection it is open on, the configuration the device granted, the
    /// levels and the dropout counts.
    pub fn link(&self) -> &DeviceLink<B, F> {
        &self.link
    }

    /// What the audio path is doing.
    pub fn state(&self) -> AudioState {
        self.link.state()
    }

    /// Why the last choice could not be opened, or `None` where the last one
    /// was.
    ///
    /// Cleared by a choice that opens, so this is about the choice in front of
    /// the player rather than the history of the run.
    pub fn refused(&self) -> Option<DeviceError> {
        self.refused
    }

    fn host(&self) -> Option<&AudioHost> {
        let named = &self.link.selection().host;

        self.listed().iter().find(|host| host.name == *named)
    }

    fn adjust(&mut self, forward: bool) {
        let Some(chosen) = self.chosen(forward) else {
            return;
        };
        if chosen == *self.link.selection() {
            return;
        }

        self.apply(chosen);
    }

    fn chosen(&self, forward: bool) -> Option<DeviceSelection> {
        match self.selected() {
            AudioSetting::Host => self.on_another_host(forward),
            AudioSetting::Input => self.on_another_input(forward),
            AudioSetting::Output => self.on_another_output(forward),
            AudioSetting::InputChannels => self.across_other_input_channels(forward),
            AudioSetting::OutputChannels => self.across_other_output_channels(forward),
        }
    }

    fn on_another_host(&self, forward: bool) -> Option<DeviceSelection> {
        let selection = self.link.selection();
        let at = self
            .listed()
            .iter()
            .position(|host| host.name == selection.host);
        let host = &self.listed()[stepped(at, self.listed().len(), forward)?];

        let input = host.inputs.first();
        let output = host.outputs.first();

        Some(DeviceSelection {
            host: host.name.clone(),
            input: input.map_or_else(|| selection.input.clone(), |device| device.id.clone()),
            input_channels: input.map_or(selection.input_channels, taken_whole),
            output: output.map_or_else(|| selection.output.clone(), |device| device.id.clone()),
            output_channels: output.map_or(selection.output_channels, taken_whole),
        })
    }

    fn on_another_input(&self, forward: bool) -> Option<DeviceSelection> {
        let selection = self.link.selection();
        let devices = &self.host()?.inputs;
        let device = &devices[stepped(found(devices, &selection.input), devices.len(), forward)?];

        Some(DeviceSelection {
            input: device.id.clone(),
            input_channels: taken_whole(device),
            ..selection.clone()
        })
    }

    fn on_another_output(&self, forward: bool) -> Option<DeviceSelection> {
        let selection = self.link.selection();
        let devices = &self.host()?.outputs;
        let device = &devices[stepped(found(devices, &selection.output), devices.len(), forward)?];

        Some(DeviceSelection {
            output: device.id.clone(),
            output_channels: taken_whole(device),
            ..selection.clone()
        })
    }

    fn across_other_input_channels(&self, forward: bool) -> Option<DeviceSelection> {
        let selection = self.link.selection();
        let devices = &self.host()?.inputs;
        let offered = runs(widest(&devices[found(devices, &selection.input)?]));
        let at = offered
            .iter()
            .position(|run| *run == selection.input_channels);

        Some(DeviceSelection {
            input_channels: offered[stepped(at, offered.len(), forward)?],
            ..selection.clone()
        })
    }

    fn across_other_output_channels(&self, forward: bool) -> Option<DeviceSelection> {
        let selection = self.link.selection();
        let devices = &self.host()?.outputs;
        let offered = runs(widest(&devices[found(devices, &selection.output)?]));
        let at = offered
            .iter()
            .position(|run| *run == selection.output_channels);

        Some(DeviceSelection {
            output_channels: offered[stepped(at, offered.len(), forward)?],
            ..selection.clone()
        })
    }

    fn apply(&mut self, chosen: DeviceSelection) {
        let running = self.link.selection().clone();

        match self.serve(chosen) {
            Ok(()) => self.refused = None,
            Err(error) => {
                self.refused = Some(error);
                let _restored = self.serve(running);
            }
        }
    }

    fn serve(&mut self, selection: DeviceSelection) -> Result<(), DeviceError> {
        self.link.select(selection)?;
        self.link.start()
    }
}

impl<B: AudioBackend, F, P> Page for AudioPage<B, F>
where
    F: FnMut() -> P,
    P: AudioPath,
{
    fn control(&mut self, event: ControlEvent) {
        match event {
            ControlEvent::Pressed {
                button: Button::Down,
                ..
            } => self.row = (self.row + 1).min(AudioSetting::ALL.len() - 1),
            ControlEvent::Pressed {
                button: Button::Up, ..
            } => self.row = self.row.saturating_sub(1),
            ControlEvent::Pressed {
                button: Button::Right,
                ..
            }
            | ControlEvent::Turned {
                encoder: Encoder::Main,
                turn: Turn::Clockwise,
                ..
            } => self.adjust(true),
            ControlEvent::Pressed {
                button: Button::Left,
                ..
            }
            | ControlEvent::Turned {
                encoder: Encoder::Main,
                turn: Turn::Anticlockwise,
                ..
            } => self.adjust(false),
            _ => {}
        }
    }

    fn legend(&self) -> Legend {
        Legend::blank()
            .answering(Button::Up)
            .answering(Button::Down)
            .answering(Button::Left)
            .answering(Button::Right)
            .answering(Encoder::Main)
    }

    fn draw(&mut self, frame: &mut Frame) {
        for (row, setting) in AudioSetting::ALL.into_iter().enumerate() {
            if row == self.row {
                frame.set(MARKER_COLUMN, row, Cell::new(MARKER));
            }
            frame.write(LABEL_COLUMN, row, label(setting));
            frame.write(VALUE_COLUMN, row, &self.value(setting));
        }

        if let Some(error) = self.refused {
            frame.write(LABEL_COLUMN, REFUSED_ROW, &format!("{REFUSED}{error}"));
        }
    }
}
