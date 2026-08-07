//! Keeping a path across the streams that play it.
//!
//! A [`DeviceLink`](super::DeviceLink) builds a path per stream it opens, and a
//! run has one of some of them: a loop engine holds the buffer the player
//! recorded into and the publishing ends of the meters, so there is no second
//! one to build. [`Escrow`] is where such a path lives between streams — lent
//! to one, back home when that one is dropped, and lent again to the next.
//!
//! Neither side of the loan is a callback's. Lending happens where a stream is
//! opened and the return where one is dropped, and both of those are the
//! [`Bench`](super::Bench)'s rather than the application thread's — which is
//! why the home is shared across threads and not merely across owners.

use std::sync::{Arc, Mutex, PoisonError};

use super::{AudioPath, Command, StreamConfig};

type Home<P> = Arc<Mutex<Option<P>>>;

/// A path kept between the streams that play it.
///
/// One path and one loan: it is out or it is home, and asking for it while it
/// is out gets nothing rather than a second path.
///
/// ```
/// use motif::audio::{AudioBackend, AudioPath, Escrow, NullBackend, Passthrough, StreamConfig,
///     StreamRequest};
///
/// let granted = StreamConfig {
///     sample_rate: 48_000,
///     block_size: 256,
///     input_channels: 2,
///     output_channels: 2,
/// };
/// let backend = NullBackend::rounding(granted);
/// let selection = backend.defaults(48_000).expect("the null backend has both directions");
/// let request = StreamRequest { sample_rate: 48_000, block_size: 256 };
///
/// let escrow = Escrow::holding(Passthrough::new());
/// let first = backend.open(&selection, request, escrow.lend()).expect("null backend opens");
/// drop(first);
///
/// let mut second = backend.open(&selection, request, escrow.lend()).expect("null backend opens");
/// let mut playing = [0.0; 2];
/// second.block(&[0.25, 0.5], &mut playing);
///
/// assert_eq!(playing, [0.25, 0.5]);
/// ```
pub struct Escrow<P> {
    home: Home<P>,
}

impl<P: AudioPath> Escrow<P> {
    /// An escrow holding `path`, having lent it to nothing yet.
    pub fn holding(path: P) -> Self {
        Self {
            home: Arc::new(Mutex::new(Some(path))),
        }
    }

    /// Lend the path to one stream.
    ///
    /// What comes back is a path in its own right, so this is what a link's
    /// builder calls. A loan taken while the path is out holds nothing and
    /// plays silence: the stream still opens, since a device that will not play
    /// the loop is better than a run with no device at all.
    pub fn lend(&self) -> Lent<P> {
        Lent {
            path: self
                .home
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
            home: Arc::clone(&self.home),
        }
    }
}

/// A path out on loan, playing for the stream it was lent to.
///
/// It goes home when the stream is dropped, which is what lets the next stream
/// have it. A loan holding nothing is silence, and returns nothing.
pub struct Lent<P> {
    path: Option<P>,
    home: Home<P>,
}

impl<P: AudioPath> AudioPath for Lent<P> {
    fn prepare(&mut self, config: StreamConfig) {
        self.path.prepare(config);
    }

    fn render(&mut self, captured: &[f32], playing: &mut [f32]) {
        self.path.render(captured, playing);
    }

    fn apply(&mut self, command: Command) -> bool {
        self.path.apply(command)
    }
}

impl<P> Drop for Lent<P> {
    /// Hands the path back, so that the stream opened after this one has it.
    ///
    /// Every route out of a stream ends here: the one a link replaces, the one
    /// a device took with it, and the one a backend refused to take at all.
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            return;
        };

        *self.home.lock().unwrap_or_else(PoisonError::into_inner) = Some(path);
    }
}
