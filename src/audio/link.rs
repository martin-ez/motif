//! Holding a stream open against a device that may go away, and the state a
//! player sees when it does.
//!
//! A stream is the wrong thing to hand the rest of the application, because a
//! device that vanishes takes it with it. What survives is the intent to play
//! through a chosen device, and whichever stream serves it: that is
//! [`DeviceLink`], and [`AudioState`] is what it looks like from outside.
//!
//! Recovery is a replacement, never a repair: a faulted stream is stopped and
//! dropped on a [`Bench`], where blocking is allowed and where the callback
//! threads are joined. [`open`](DeviceLink::open) replaces the stream whatever
//! the reason and [`select`](DeviceLink::select) is that call after changing
//! the choice, so a lost device and a changed one are one mechanism.
//!
//! Neither waits for the device. The answer comes back at
//! [`poll`](DeviceLink::poll), which a caller already runs once a frame, or at
//! [`settled`](DeviceLink::settled) where there is nothing to draw meanwhile.
//!
//! One run, one device, one link: [`SharedLink`] is how its holders share it.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::{
    AudioBackend, AudioPath, Bench, DeviceError, DeviceSelection, DuplexStream, GUARDED_LEVEL,
    Opening, StreamRequest,
};

const UNITY: f32 = 1.0;

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
    /// A stream is being opened, and the device has not answered.
    ///
    /// [`DeviceLink::choose`] reaches it without touching the device, keeping
    /// whatever was playing; [`DeviceLink::open`] reaches it by handing the
    /// work to a bench, which is where that stream is torn down. Taking the
    /// answer is what leaves it, whichever answer comes.
    Opening,
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
            Self::Opening => f.write_str("opening"),
            Self::Lost(why) => write!(f, "lost: {why}"),
        }
    }
}

/// A duplex stream, and the backend, request and path needed to open another
/// one like it.
///
/// Nothing here runs on the audio thread. It is the application-thread end of
/// the boundary: it reads the fault a callback latched, and it decides the
/// stopping, dropping and rebuilding that a callback may not. Deciding is all
/// it does on that thread — the device work itself goes to the bench.
///
/// Only opening a stream needs to know what the path is, so whatever holds a
/// link can poll, stop and drop one without naming what it plays.
pub struct DeviceLink<B: AudioBackend, F> {
    backend: Arc<B>,
    request: StreamRequest,
    selection: DeviceSelection,
    path: Arc<F>,
    stream: Option<B::Stream>,
    state: AudioState,
    chosen: bool,
    running: bool,
    bench: Bench,
    awaiting: Option<Receiver<Result<B::Stream, DeviceError>>>,
}

impl<B: AudioBackend, F, P> DeviceLink<B, F>
where
    F: Fn() -> P + Send + Sync + 'static,
    P: AudioPath,
{
    /// A link that will open `selection` at `request` on `backend`, playing
    /// through what `path` builds, and having opened nothing yet.
    ///
    /// A path is built rather than held, because opening a stream moves one to
    /// where the callback can reach it: every stream the link opens gets one of
    /// its own, and [`Escrow`](super::Escrow) is what builds those out of a path
    /// a run has only one of. The bench builds it, after the stream being
    /// replaced is dropped — which is when an escrow has its path back to lend.
    ///
    /// Touches no device, so this cannot fail; the first
    /// [`open`](Self::open) is where a device gets a say.
    pub fn new(backend: B, request: StreamRequest, selection: DeviceSelection, path: F) -> Self {
        Self {
            backend: Arc::new(backend),
            request,
            selection,
            path: Arc::new(path),
            stream: None,
            state: AudioState::Closed,
            chosen: false,
            running: false,
            bench: Bench::new(),
            awaiting: None,
        }
    }

    /// Hand the bench a stream to open, replacing whichever one the link is
    /// holding.
    ///
    /// Returns without waiting for the device, leaving the link
    /// [`AudioState::Opening`]: a caller that opens a device from a frame gets
    /// the frame back. The bench stops and drops the replaced stream before it
    /// opens the new one, so no two streams are ever live on one link and the
    /// old callbacks have finished before the new ones begin.
    ///
    /// An open already in flight finishes first, so there is one at a time.
    /// This is the way out of [`AudioState::Lost`], and also what changing
    /// device comes down to; [`poll`](Self::poll) is where either lands.
    pub fn open(&mut self) {
        self.settled();

        let backend = Arc::clone(&self.backend);
        let selection = self.selection.clone();
        let request = self.request;
        let build = Arc::clone(&self.path);
        let level = self.opening_level();
        let mut replaced = self.stream.take();
        let (answer, awaiting) = channel();

        self.bench.run(move || {
            if let Some(stream) = replaced.as_mut() {
                let _stopped = stream.stop();
            }
            drop(replaced);

            let path = Opening::at(level, build());
            let _answered = answer.send(backend.open(&selection, request, path));
        });

        self.awaiting = Some(awaiting);
        self.state = AudioState::Opening;
    }

    /// Take `selection` as the choice to serve, and open it.
    ///
    /// The link holds one stream at a time, so the one that was running stops
    /// and is dropped before the new one opens: a device serving both cannot be
    /// opened twice, and a moment of silence is the price of the change.
    ///
    /// As [`open`](Self::open), this does not wait for the device. The refused
    /// selection is kept, so a link that lands in [`AudioState::Lost`] reports
    /// what was asked for, not what came before.
    pub fn select(&mut self, selection: DeviceSelection) {
        self.choose(selection);
        self.open();
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

    /// The multiplier every stream this link opens comes up to.
    ///
    /// Unity where a player chose the devices, and [`GUARDED_LEVEL`] where
    /// nobody has: a link opened on what the backend offered does not know what
    /// it is playing into, and on a laptop that is a microphone in front of the
    /// speakers it is about to play through. Choosing is
    /// [`select`](Self::select), whether or not the device took it — a refused
    /// choice is still one, and reopening the same selection is not.
    pub fn opening_level(&self) -> f32 {
        if self.chosen { UNITY } else { GUARDED_LEVEL }
    }

    /// Take `selection` as the choice to serve, without opening it.
    ///
    /// The link reports [`AudioState::Opening`] and whatever is running keeps
    /// running, because choosing a device is not what silences the one before
    /// it. [`open`](Self::open) is what serves the choice, and
    /// [`select`](Self::select) is the two of them together.
    pub fn choose(&mut self, selection: DeviceSelection) {
        self.selection = selection;
        self.chosen = true;
        self.state = AudioState::Opening;
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

    /// Wait for the open in flight, and report what the link is doing.
    ///
    /// Blocking, so this belongs where there is no frame to give back: a run
    /// opening its device before the screen exists, or a caller with nothing to
    /// draw meanwhile. [`poll`](Self::poll) is the same answer without the
    /// wait. A bench that died holding the work reports
    /// [`DeviceError::BackendFailure`].
    pub fn settled(&mut self) -> AudioState {
        if let Some(awaiting) = self.awaiting.take() {
            let answer = awaiting.recv().unwrap_or(Err(DeviceError::BackendFailure));
            self.arrive(answer);
        }

        self.state
    }

    /// Stop and drop the stream, leaving the link [`AudioState::Closed`].
    ///
    /// A stream that refuses to stop is dropped anyway. There is nothing left
    /// to do with one, and its own drop is what waits for the callbacks. The
    /// stream an open in flight is about to hand over is waited for and dropped
    /// too, here rather than on the bench: closing is what a run does on its
    /// way out, and there is no frame left to protect.
    pub fn close(&mut self) {
        self.settled();

        if let Some(stream) = self.stream.as_mut() {
            let _ = stream.stop();
        }
        self.stream = None;
        self.state = AudioState::Closed;
        self.running = false;
    }

    /// Start calling back, and keep calling back across the streams that
    /// follow.
    ///
    /// A link being opened has no stream to start, so this records that it is
    /// wanted playing and the stream the bench hands over starts on arrival.
    /// That is what lets a caller say "play this device" in the frame that
    /// chose it rather than in whichever later frame the device answered on.
    ///
    /// # Errors
    ///
    /// Returns the latched [`DeviceError`] where the device is already lost,
    /// [`DeviceError::DeviceNotAvailable`] where no stream is open and none is
    /// being opened, and whatever the device refuses with otherwise. A refusal
    /// moves the link to [`AudioState::Lost`], because a stream that will not
    /// start is not one anything can wait on.
    pub fn start(&mut self) -> Result<(), DeviceError> {
        self.running = true;
        self.act(DuplexStream::start, AudioState::Playing)
    }

    /// Stop calling back, leaving the stream open, and leave the streams that
    /// follow stopped too.
    ///
    /// # Errors
    ///
    /// As [`start`](Self::start), and for the same reasons.
    pub fn stop(&mut self) -> Result<(), DeviceError> {
        self.running = false;
        self.act(DuplexStream::stop, AudioState::Idle)
    }

    /// Take whatever happened between frames, and report what the link is doing
    /// now.
    ///
    /// Two things happen there. A device the bench finished opening arrives,
    /// and starts if the link is wanted playing. A fault latched on the audio
    /// thread becomes [`AudioState::Lost`], the faulted stream stopped and
    /// dropped and the reason it gave kept. Call it wherever the application
    /// already looks at its audio path — once a frame is plenty, since neither
    /// answer goes anywhere.
    ///
    /// Cheap and idempotent when nothing has happened: one atomic load.
    pub fn poll(&mut self) -> AudioState {
        self.collect();

        if let Some(fault) = self.stream.as_ref().and_then(DuplexStream::fault) {
            self.close();
            self.lose(fault);
        }
        self.state
    }

    fn collect(&mut self) {
        let Some(awaiting) = self.awaiting.as_ref() else {
            return;
        };

        match awaiting.try_recv() {
            Ok(answer) => {
                self.awaiting = None;
                self.arrive(answer);
            }
            Err(TryRecvError::Disconnected) => {
                self.awaiting = None;
                self.arrive(Err(DeviceError::BackendFailure));
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn arrive(&mut self, answer: Result<B::Stream, DeviceError>) {
        match answer {
            Ok(stream) => {
                self.stream = Some(stream);
                self.state = AudioState::Idle;

                if self.running {
                    let _playing = self.start();
                }
            }
            Err(error) => {
                self.lose(error);
            }
        }
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
            return match self.state {
                AudioState::Opening => Ok(()),
                _ => Err(DeviceError::DeviceNotAvailable),
            };
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
/// link.change(DeviceLink::open);
/// link.change(DeviceLink::settled);
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
    F: Fn() -> P + Send + Sync + 'static,
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
