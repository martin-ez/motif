//! Holding a stream open against a device that may go away, and the state a
//! player sees when it does.
//!
//! A stream is the wrong thing to hand the rest of the application, because a
//! device that vanishes takes it with it. What survives is the intent to play
//! through a chosen device, and whichever stream serves it: that is
//! [`DeviceLink`], and [`AudioState`] is what it looks like from outside.
//!
//! Recovery is a replacement, never a repair: a faulted stream is stopped and
//! dropped on the application thread, where dropping is allowed and is what
//! joins the callback threads. [`open`](DeviceLink::open) replaces the stream
//! whatever the reason and [`select`](DeviceLink::select) is that call after
//! changing the choice, so a lost device and a changed one are one mechanism.
//!
//! One run, one device, one link: [`SharedLink`] is how its holders share it.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use super::{AudioBackend, AudioPath, DeviceError, DeviceSelection, DuplexStream, StreamRequest};

/// What the audio path is doing, as far as the rest of the application is
/// concerned.
///
/// This is the value a UI renders. It answers "can I play right now, and if
/// not, why not" in one match, which is the question a status line is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioState {
    /// No stream is open, because none has been opened or one was closed.
    Closed,
    /// A stream is open and not calling back.
    Idle,
    /// A stream is open and calling back.
    Playing,
    /// The device failed, and this is what it failed with.
    ///
    /// The stream is gone. [`DeviceLink::open`] is the way out of this state,
    /// and it is the only one — nothing recovers on its own.
    Lost(DeviceError),
}

impl fmt::Display for AudioState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("closed"),
            Self::Idle => f.write_str("idle"),
            Self::Playing => f.write_str("playing"),
            Self::Lost(why) => write!(f, "lost: {why}"),
        }
    }
}

/// A duplex stream, and the backend, request and path needed to open another
/// one like it.
///
/// Nothing here runs on the audio thread. It is the application-thread end of
/// the boundary: it reads the fault a callback latched, and it does the
/// stopping, dropping and rebuilding that a callback may not.
///
/// Only opening a stream needs to know what the path is, so whatever holds a
/// link can poll, stop and drop one without naming what it plays.
pub struct DeviceLink<B: AudioBackend, F> {
    backend: B,
    request: StreamRequest,
    selection: DeviceSelection,
    path: F,
    stream: Option<B::Stream>,
    state: AudioState,
}

impl<B: AudioBackend, F, P> DeviceLink<B, F>
where
    F: FnMut() -> P,
    P: AudioPath,
{
    /// A link that will open `selection` at `request` on `backend`, playing
    /// through what `path` builds, and having opened nothing yet.
    ///
    /// A path is built rather than held, because opening a stream moves one to
    /// where the callback can reach it: every stream the link opens gets one of
    /// its own, and [`Escrow`](super::Escrow) is what builds those out of a path
    /// a run has only one of.
    ///
    /// Touches no device, so this cannot fail; the first
    /// [`open`](Self::open) is where a device gets a say.
    pub fn new(backend: B, request: StreamRequest, selection: DeviceSelection, path: F) -> Self {
        Self {
            backend,
            request,
            selection,
            path,
            stream: None,
            state: AudioState::Closed,
        }
    }

    /// Open a stream, replacing whichever one the link is holding.
    ///
    /// The replaced stream is stopped and dropped before the new one is opened,
    /// so no two streams are ever live on one link and the old callbacks have
    /// finished before the new ones begin. This is the way out of
    /// [`AudioState::Lost`], and also what changing device comes down to.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError`] when the backend will not open the request, and
    /// leaves the link in [`AudioState::Lost`] carrying it.
    pub fn open(&mut self) -> Result<(), DeviceError> {
        self.close();

        let path = (self.path)();
        match self.backend.open(&self.selection, self.request, path) {
            Ok(stream) => {
                self.stream = Some(stream);
                self.state = AudioState::Idle;
                Ok(())
            }
            Err(error) => Err(self.lose(error)),
        }
    }

    /// Take `selection` as the choice to serve, and open it.
    ///
    /// The link holds one stream at a time, so the one that was running stops
    /// and is dropped before the new one opens: a device serving both cannot be
    /// opened twice, and a moment of silence is the price of the change.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open). The refused selection is kept, so a link left
    /// in [`AudioState::Lost`] reports what was asked for, not what came before.
    pub fn select(&mut self, selection: DeviceSelection) -> Result<(), DeviceError> {
        self.selection = selection;
        self.open()
    }
}

impl<B: AudioBackend, F> DeviceLink<B, F> {
    /// What the audio path is doing, as of the last [`poll`](Self::poll).
    ///
    /// A device that went away since then still reads as whatever it was doing
    /// before it did: a fault is latched on the audio thread and noticed on
    /// this one.
    pub fn state(&self) -> AudioState {
        self.state
    }

    /// The configuration every stream this link opens is asked for.
    pub fn request(&self) -> StreamRequest {
        self.request
    }

    /// The backend every stream this link opens comes from.
    ///
    /// Lent rather than cloned, so a [`DeviceCatalog`](super::DeviceCatalog)
    /// can be refreshed through the same backend the link is playing through.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// The devices and channels the link is opening, or last tried to.
    ///
    /// A selection a device refused stays here rather than being rolled back,
    /// so a link in [`AudioState::Lost`] says both what was tried and why it
    /// failed, and [`open`](Self::open) is a retry of the same thing.
    pub fn selection(&self) -> &DeviceSelection {
        &self.selection
    }

    /// The stream currently serving the link, or `None` where none is open.
    ///
    /// This is the route to what a stream knows and the link does not: the
    /// configuration the device granted, the levels, the dropout counts, the
    /// callback's headroom.
    pub fn stream(&self) -> Option<&B::Stream> {
        self.stream.as_ref()
    }

    /// Stop and drop the stream, leaving the link [`AudioState::Closed`].
    ///
    /// A stream that refuses to stop is dropped anyway. There is nothing left
    /// to do with one, and its own drop is what waits for the callbacks.
    pub fn close(&mut self) {
        if let Some(stream) = self.stream.as_mut() {
            let _ = stream.stop();
        }
        self.stream = None;
        self.state = AudioState::Closed;
    }

    /// Start calling back.
    ///
    /// # Errors
    ///
    /// Returns the latched [`DeviceError`] where the device is already lost,
    /// [`DeviceError::DeviceNotAvailable`] where no stream is open, and
    /// whatever the device refuses with otherwise. A refusal moves the link to
    /// [`AudioState::Lost`], because a stream that will not start is not one
    /// anything can wait on.
    pub fn start(&mut self) -> Result<(), DeviceError> {
        self.act(DuplexStream::start, AudioState::Playing)
    }

    /// Stop calling back, leaving the stream open.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start), and for the same reasons.
    pub fn stop(&mut self) -> Result<(), DeviceError> {
        self.act(DuplexStream::stop, AudioState::Idle)
    }

    /// Notice a device that failed, and report what the link is doing now.
    ///
    /// This is what turns a fault latched on the audio thread into
    /// [`AudioState::Lost`]: the faulted stream is stopped and dropped here,
    /// and the reason it gave is kept. Call it wherever the application already
    /// looks at its audio path — once a frame is plenty, since the fault waits.
    ///
    /// Cheap and idempotent when nothing has failed: one atomic load.
    pub fn poll(&mut self) -> AudioState {
        if let Some(fault) = self.stream.as_ref().and_then(DuplexStream::fault) {
            self.close();
            self.lose(fault);
        }
        self.state
    }

    fn act(
        &mut self,
        action: fn(&mut B::Stream) -> Result<(), DeviceError>,
        reached: AudioState,
    ) -> Result<(), DeviceError> {
        if let AudioState::Lost(error) = self.state {
            return Err(error);
        }

        let Some(stream) = self.stream.as_mut() else {
            return Err(DeviceError::DeviceNotAvailable);
        };

        match action(stream) {
            Ok(()) => {
                self.state = reached;
                Ok(())
            }
            Err(error) => {
                self.close();
                Err(self.lose(error))
            }
        }
    }

    fn lose(&mut self, error: DeviceError) -> DeviceError {
        self.state = AudioState::Lost(error);
        error
    }
}

/// One [`DeviceLink`], held by everything in a run that reaches the device.
///
/// Both ends sit on the application thread — the event loop calls them in turn,
/// so there is nothing here to cross and the shared cell says so. What it
/// guards is two owners rather than two threads: a run holding a link each
/// would open two streams on one interface and disagree about which is playing.
///
/// Access is scoped to a closure, and [`change`](Self::change) takes the handle
/// by `&mut`, so no borrow outlives its call or is reached from inside another.
///
/// ```
/// use motif::audio::{
///     AudioState, DeviceLink, NullBackend, Passthrough, SharedLink, StreamConfig, StreamRequest,
/// };
///
/// let granted = StreamConfig {
///     sample_rate: 48_000,
///     block_size: 256,
///     input_channels: 2,
///     output_channels: 2,
/// };
/// let request = StreamRequest { sample_rate: 48_000, block_size: 256 };
///
/// let mut link = SharedLink::defaulting(NullBackend::rounding(granted), request, Passthrough::new)
///     .expect("the null backend has a device in each direction");
/// let watching = link.clone();
///
/// let _opened = link.change(DeviceLink::open);
///
/// assert_eq!(watching.read(DeviceLink::state), AudioState::Idle);
/// ```
pub struct SharedLink<B: AudioBackend, F> {
    link: Rc<RefCell<DeviceLink<B, F>>>,
}

impl<B: AudioBackend, F> Clone for SharedLink<B, F> {
    /// Another handle on the same link, never a second link.
    fn clone(&self) -> Self {
        Self {
            link: Rc::clone(&self.link),
        }
    }
}

impl<B: AudioBackend, F, P> SharedLink<B, F>
where
    F: FnMut() -> P,
    P: AudioPath,
{
    /// A shared link on what `backend` would open at `request` if nobody chose,
    /// or `None` where it has nothing to offer.
    ///
    /// Touches no device beyond listing them, so what comes back has opened
    /// nothing: the first [`DeviceLink::open`] through it is where a device
    /// gets a say.
    pub fn defaulting(backend: B, request: StreamRequest, path: F) -> Option<Self> {
        let selection = backend.defaults(request.sample_rate)?;

        Some(Self::new(DeviceLink::new(
            backend, request, selection, path,
        )))
    }
}

impl<B: AudioBackend, F> SharedLink<B, F> {
    /// Share `link`, so that more than one part of a run can reach it.
    pub fn new(link: DeviceLink<B, F>) -> Self {
        Self {
            link: Rc::new(RefCell::new(link)),
        }
    }

    /// Read the link through `f`, and report what `f` returned.
    ///
    /// Reads nest, so a caller that reaches the link twice over one expression
    /// is fine. Nothing borrowed from the link escapes: `f` returns a value.
    pub fn read<R>(&self, f: impl FnOnce(&DeviceLink<B, F>) -> R) -> R {
        f(&self.link.borrow())
    }

    /// Change the link through `f`, and report what `f` returned.
    ///
    /// The route to opening, starting, stopping and polling. It takes the
    /// handle by `&mut` so that a change cannot be reached from inside a read
    /// or another change on the same one.
    pub fn change<R>(&mut self, f: impl FnOnce(&mut DeviceLink<B, F>) -> R) -> R {
        f(&mut self.link.borrow_mut())
    }
}
