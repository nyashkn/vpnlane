//! The thread that runs effects so the UI thread never blocks on I/O.
//!
//! The reducer stays pure: it returns [`Effect`]s, the shell [`Worker::send`]s
//! them here, and each result comes back through [`Worker::try_recv`] as an
//! [`Event`] for the reducer. Effects run one at a time, in order, so a
//! connect is never interleaved with a status refresh.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use super::app::{Effect, Event};

/// Handle to the worker thread.
pub(super) struct Worker {
    tx: Sender<Effect>,
    rx: Receiver<Event>,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl Worker {
    /// Starts the thread. `exec` runs one effect and calls its second argument
    /// once per resulting event (a probe emits one per server).
    pub(super) fn spawn(
        mut exec: impl FnMut(Effect, &mut dyn FnMut(Event)) + Send + 'static,
    ) -> Self {
        let (tx, effects) = mpsc::channel::<Effect>();
        let (events, rx) = mpsc::channel::<Event>();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while let Ok(effect) = effects.recv() {
                // Checked between effects only: one in flight always completes.
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                exec(effect, &mut |e| {
                    let _ = events.send(e);
                });
            }
        });
        Self {
            tx,
            rx,
            stop,
            handle,
        }
    }

    /// Queues an effect. Dropped silently if the worker has ended.
    pub(super) fn send(&self, effect: Effect) {
        let _ = self.tx.send(effect);
    }

    /// The next finished result, if any.
    pub(super) fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// Ends the worker: effects not yet started are skipped, the one running
    /// (if any) finishes. With `wait` this blocks until then and returns the
    /// events it produced; without, the thread is left to die with the process
    /// (only for read-only effects, never a tunnel change).
    pub(super) fn shutdown(self, wait: bool) -> Vec<Event> {
        self.stop.store(true, Ordering::SeqCst);
        drop(self.tx);
        if !wait {
            return Vec::new();
        }
        let _ = self.handle.join();
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use vpnlane_core::CountryCode;

    use super::*;

    fn cc() -> CountryCode {
        "US".parse().unwrap()
    }

    /// Waits for the next event; the channel makes this deterministic.
    fn next(w: &Worker) -> Event {
        for _ in 0..2000 {
            if let Some(e) = w.try_recv() {
                return e;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("no event within 10 s");
    }

    fn server(host: &str) -> vpnlane_core::Server {
        vpnlane_core::Server {
            hostname: host.into(),
            station: "203.0.113.1".parse().unwrap(),
            load: 1,
            public_key: vpnlane_core::PublicKey(String::new()),
            country: cc(),
            city: None,
        }
    }

    #[test]
    fn runs_effects_in_order_and_streams_each_event() {
        let w = Worker::spawn(|effect, emit| match effect {
            Effect::Connect(..) => emit(Event::Finished(Ok(()))),
            Effect::Probe { epoch, servers } => {
                for s in servers {
                    emit(Event::ProbeResult {
                        epoch,
                        key: s.hostname,
                        latency_ms: Some(1),
                    });
                }
            }
            _ => emit(Event::Tick),
        });
        w.send(Effect::Refresh);
        w.send(Effect::Connect(cc(), None));
        w.send(Effect::Probe {
            epoch: 7,
            servers: vec![server("a"), server("b")],
        });
        assert_eq!(next(&w), Event::Tick);
        assert_eq!(next(&w), Event::Finished(Ok(())));
        for host in ["a", "b"] {
            let want = Event::ProbeResult {
                epoch: 7,
                key: host.into(),
                latency_ms: Some(1),
            };
            assert_eq!(next(&w), want);
        }
        assert_eq!(w.shutdown(true), Vec::new());
    }

    #[test]
    fn shutdown_lets_the_running_effect_finish_and_skips_the_rest() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (started_tx, started) = mpsc::channel();
        let (release, gate) = mpsc::channel::<()>();
        let seen = Arc::clone(&log);
        let w = Worker::spawn(move |effect, emit| {
            seen.lock().unwrap().push(format!("{effect:?}"));
            if matches!(effect, Effect::Connect(..)) {
                started_tx.send(()).unwrap();
                gate.recv().unwrap();
                emit(Event::Finished(Ok(())));
            }
        });
        w.send(Effect::Connect(cc(), None));
        started.recv().unwrap();
        // The connect is in flight; a refresh waits behind it; then we quit.
        w.send(Effect::Refresh);
        w.stop.store(true, Ordering::SeqCst);
        release.send(()).unwrap();
        let events = w.shutdown(true);
        assert_eq!(events, [Event::Finished(Ok(()))], "the connect completed");
        assert_eq!(log.lock().unwrap().len(), 1, "the queued refresh never ran");
    }

    #[test]
    fn shutdown_without_waiting_returns_at_once() {
        let (started_tx, started) = mpsc::channel();
        let (release, gate) = mpsc::channel::<()>();
        let w = Worker::spawn(move |_, _| {
            started_tx.send(()).unwrap();
            let _ = gate.recv();
        });
        w.send(Effect::Refresh);
        started.recv().unwrap();
        assert_eq!(w.shutdown(false), Vec::new());
        release.send(()).unwrap();
    }
}
