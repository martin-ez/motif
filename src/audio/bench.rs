//! Running the work of opening a device somewhere other than the frame that
//! asked for it.
//!
//! Opening a stream is a teardown, an enumeration, four blocking queries and
//! two stream builds, and a host takes a fraction of a second over it. A frame
//! that does that overruns, so [`Bench`] is where it happens instead: one
//! worker thread, running what it is handed in the order it was handed it.
//!
//! A closure rather than a message, because the work is generic in the backend,
//! the stream and the path it plays. Built where those types are known and
//! boxed on the way out, the channel between the two threads names none of
//! them.

use std::sync::mpsc::{Sender, channel};
use std::thread::{self, JoinHandle};

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A worker thread that runs what it is handed, one job at a time.
///
/// One worker rather than a thread per job, so the jobs are ordered: opening a
/// device and dropping the stream it replaces are the same device's work, and
/// two of them at once is two streams live on one interface.
///
/// ```
/// use std::sync::mpsc::channel;
///
/// use motif::audio::Bench;
///
/// let (done, answers) = channel();
/// let bench = Bench::new();
///
/// bench.run(move || {
///     let _sent = done.send("opened");
/// });
///
/// assert_eq!(answers.recv(), Ok("opened"));
/// ```
pub struct Bench {
    work: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}

impl Bench {
    /// A bench with a worker thread of its own, idle until it is given work.
    pub fn new() -> Self {
        let (work, jobs) = channel::<Job>();
        let worker = thread::spawn(move || {
            for job in jobs {
                job();
            }
        });

        Self {
            work: Some(work),
            worker: Some(worker),
        }
    }

    /// Hand `job` to the worker, to run after everything handed over before it.
    ///
    /// Returns without waiting, which is the whole point: the caller is a frame
    /// and the job is a device. A bench whose worker has gone drops the job
    /// rather than running it, so whatever the job was going to answer sees a
    /// channel with no sender left.
    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        if let Some(work) = self.work.as_ref() {
            let _handed = work.send(Box::new(job));
        }
    }
}

impl Default for Bench {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Bench {
    /// Wait for the work already handed over.
    ///
    /// A job holds the stream it is replacing and the one it opened, and each
    /// of those has callback threads its drop joins. Going out from under one
    /// would leave a device open with nothing left to close it.
    fn drop(&mut self) {
        self.work = None;

        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}
