//! Handing work to a worker thread and getting it back, which is how opening a
//! device stops costing the frame that asked for it.
//!
//! The bench carries closures rather than messages, so what a test hands over
//! is whatever it wants to observe: a value down a channel, the identity of the
//! thread it ran on, or a flag set on the way out.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::thread::{self, ThreadId};
use std::time::Duration;

use motif::audio::Bench;

const SLOW: Duration = Duration::from_millis(50);

#[test]
fn a_bench_runs_what_it_is_handed() {
    let (done, answers) = channel();
    let bench = Bench::new();

    bench.run(move || {
        let _sent = done.send("opened");
    });

    assert_eq!(answers.recv(), Ok("opened"));
}

#[test]
fn a_bench_runs_its_work_away_from_the_caller() {
    let (done, answers) = channel::<ThreadId>();
    let bench = Bench::new();

    bench.run(move || {
        let _sent = done.send(thread::current().id());
    });

    assert_ne!(
        answers.recv().expect("the bench answers"),
        thread::current().id()
    );
}

#[test]
fn jobs_run_in_the_order_they_were_handed_over() {
    let (done, answers) = channel();
    let second = done.clone();
    let bench = Bench::new();

    bench.run(move || {
        thread::sleep(SLOW);
        let _sent = done.send("first");
    });
    bench.run(move || {
        let _sent = second.send("second");
    });

    assert_eq!(answers.recv(), Ok("first"));
    assert_eq!(answers.recv(), Ok("second"));
}

#[test]
fn every_job_runs_on_the_one_worker() {
    let (done, answers) = channel::<ThreadId>();
    let second = done.clone();
    let bench = Bench::new();

    bench.run(move || {
        let _sent = done.send(thread::current().id());
    });
    bench.run(move || {
        let _sent = second.send(thread::current().id());
    });

    assert_eq!(
        answers.recv().expect("the bench answers"),
        answers.recv().expect("the bench answers twice")
    );
}

#[test]
fn dropping_a_bench_waits_for_the_work_it_was_given() {
    let finished = Arc::new(AtomicBool::new(false));
    let reported = Arc::clone(&finished);
    let bench = Bench::new();

    bench.run(move || {
        thread::sleep(SLOW);
        reported.store(true, Ordering::Relaxed);
    });
    drop(bench);

    assert!(finished.load(Ordering::Relaxed));
}
