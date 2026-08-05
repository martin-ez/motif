//! Holding a stream open against a device that may go away, and the state a
//! player sees when it does.
//!
//! A stream is the wrong thing to hand the rest of the application, because a
//! device that vanishes takes it with it. What survives is the intent to be
//! playing through *some* device — the backend, the configuration asked of it,
//! and whichever stream is currently serving that intent. That is
//! [`DeviceLink`], and [`AudioState`] is what it looks like from outside.
//!
//! Recovery is a replacement, never a repair: a faulted stream is stopped and
//! dropped on the application thread, where dropping is allowed and is what
//! joins the callback threads. [`open`](DeviceLink::open) replaces the stream
//! whatever the reason, so a lost device and a changed choice are one mechanism.

use std::fmt;

use super::{AudioBackend, DeviceError, DuplexStream, StreamRequest};

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

/// A duplex stream, and the backend and request needed to open another one like
/// it.
///
/// Nothing here runs on the audio thread. It is the application-thread end of
/// the boundary: it reads the fault a callback latched, and it does the
/// stopping, dropping and rebuilding that a callback may not.
pub struct DeviceLink<B: AudioBackend> {
    backend: B,
    request: StreamRequest,
    stream: Option<B::Stream>,
    state: AudioState,
}

impl<B: AudioBackend> DeviceLink<B> {
    /// A link that will open `request` on `backend`, having opened nothing yet.
    ///
    /// Touches no device, so this cannot fail; the first
    /// [`open`](Self::open) is where a device gets a say.
    pub fn new(backend: B, request: StreamRequest) -> Self {
        Self {
            backend,
            request,
            stream: None,
            state: AudioState::Closed,
        }
    }

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

    /// The stream currently serving the link, or `None` where none is open.
    ///
    /// This is the route to what a stream knows and the link does not: the
    /// configuration the device granted, the levels, the dropout counts, the
    /// callback's headroom.
    pub fn stream(&self) -> Option<&B::Stream> {
        self.stream.as_ref()
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

        match self.backend.open(self.request) {
            Ok(stream) => {
                self.stream = Some(stream);
                self.state = AudioState::Idle;
                Ok(())
            }
            Err(error) => Err(self.lose(error)),
        }
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
