use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use wglane_core::ports::NetworkWatcher;
use wglane_core::{Error, Result};

/// Quiet time after the last event before a burst counts as one change.
const QUIET: Duration = Duration::from_secs(1);
/// A burst never delays the report longer than this, so a chatty network
/// cannot starve the caller.
const MAX_BURST: Duration = Duration::from_secs(5);

/// [`NetworkWatcher`] over `route -n monitor`, which streams routing socket
/// messages: push events, no polling.
#[derive(Debug, Default)]
pub struct RouteMonitor {
    running: Option<Running>,
}

#[derive(Debug)]
struct Running {
    child: Child,
    events: Receiver<()>,
}

impl RouteMonitor {
    /// Not started until the first `wait_for_change`.
    pub fn new() -> Self {
        Self::default()
    }

    fn start() -> Result<Running> {
        let mut child = Command::new("/sbin/route")
            .args(["-n", "monitor"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::io("starting `route -n monitor`", e))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            Error::io(
                "starting `route -n monitor`",
                std::io::ErrorKind::BrokenPipe.into(),
            )
        })?;
        let (tx, events) = mpsc::channel();
        // Blocking reads cannot time out, so a thread turns lines into events
        // and the caller debounces with `recv_timeout`.
        thread::spawn(move || {
            let mut filter = RouteFilter::default();
            for line in BufReader::new(stdout)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if filter.feed(&line) && tx.send(()).is_err() {
                    break;
                }
            }
        });
        Ok(Running { child, events })
    }
}

impl Drop for RouteMonitor {
    fn drop(&mut self) {
        if let Some(mut r) = self.running.take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }
}

impl NetworkWatcher for RouteMonitor {
    fn wait_for_change(&mut self) -> Result<()> {
        if self.running.is_none() {
            self.running = Some(Self::start()?);
        }
        let running = self.running.as_ref().expect("started just above");
        collect_burst(&running.events, QUIET, MAX_BURST).map_err(|()| {
            self.running = None;
            Error::io(
                "`route -n monitor` exited; restart `wglane watch`",
                std::io::ErrorKind::UnexpectedEof.into(),
            )
        })
    }
}

/// Blocks for the first event, then swallows the rest of the burst.
/// `Err(())` when the sender is gone.
fn collect_burst(events: &Receiver<()>, quiet: Duration, max: Duration) -> Result<(), ()> {
    events.recv().map_err(|_| ())?;
    let first = Instant::now();
    let mut last = first;
    while let Some(wait) = debounce_wait(first.elapsed(), last.elapsed(), quiet, max) {
        match events.recv_timeout(wait) {
            Ok(()) => last = Instant::now(),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

/// How much longer to wait for more events, or `None` when the burst is over:
/// quiet for `quiet` since the last event, or `max` since the first.
fn debounce_wait(
    since_first: Duration,
    since_last: Duration,
    quiet: Duration,
    max: Duration,
) -> Option<Duration> {
    let until_quiet = quiet.checked_sub(since_last)?;
    let until_max = max.checked_sub(since_first)?;
    Some(until_quiet.min(until_max)).filter(|d| !d.is_zero())
}

/// Picks the `route -n monitor` messages that mean "the way out changed".
///
/// A message is a header line (`RTM_ADD: ...`) followed by detail lines whose
/// last one is the destination row. Only the default route and interface
/// changes count; our own bypass routes are host routes and are ignored.
#[derive(Debug, Default)]
struct RouteFilter {
    /// Inside an `RTM_ADD`/`RTM_DELETE`/`RTM_CHANGE` message, destination not seen yet.
    pending_route: bool,
}

impl RouteFilter {
    /// True when `line` completes a relevant change.
    fn feed(&mut self, line: &str) -> bool {
        let line = line.trim();
        if line.starts_with("RTM_IFINFO") {
            self.pending_route = false;
            return true;
        }
        if line.starts_with("got message") {
            self.pending_route = false;
        } else if line.starts_with("RTM_ADD")
            || line.starts_with("RTM_DELETE")
            || line.starts_with("RTM_CHANGE")
        {
            self.pending_route = true;
        } else if self.pending_route && (line.starts_with("default") || line.starts_with("0.0.0.0"))
        {
            self.pending_route = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: Duration = Duration::from_secs(1);
    const MAX: Duration = Duration::from_secs(5);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn debounce_waits_out_the_quiet_window() {
        assert_eq!(debounce_wait(ms(0), ms(0), Q, MAX), Some(Q));
        assert_eq!(debounce_wait(ms(300), ms(300), Q, MAX), Some(ms(700)));
    }

    #[test]
    fn debounce_ends_when_quiet() {
        assert_eq!(debounce_wait(ms(1500), Q, Q, MAX), None);
        assert_eq!(debounce_wait(ms(1500), ms(2000), Q, MAX), None);
    }

    #[test]
    fn debounce_is_capped_for_endless_bursts() {
        assert_eq!(debounce_wait(ms(4800), ms(10), Q, MAX), Some(ms(200)));
        assert_eq!(debounce_wait(MAX, ms(10), Q, MAX), None);
    }

    #[test]
    fn filter_reports_default_route_changes_once() {
        let mut f = RouteFilter::default();
        let lines = [
            "got message of size 240 on Fri Oct 30 10:00:00 2026",
            "RTM_ADD: Add Route: len 240, pid: 0, seq 1, errno 0, flags:<UP,GATEWAY,DONE,STATIC>",
            "locks:  inits:",
            "sockaddrs: <DST,GATEWAY,NETMASK>",
            " default 192.168.1.1 default",
        ];
        let hits: Vec<bool> = lines.iter().map(|l| f.feed(l)).collect();
        assert_eq!(hits, [false, false, false, false, true]);
    }

    #[test]
    fn filter_ignores_host_routes_and_resets_per_message() {
        let mut f = RouteFilter::default();
        assert!(!f.feed("RTM_ADD: Add Route: len 240"));
        assert!(!f.feed(" 100.64.0.1 192.168.1.1 255.255.255.255"));
        assert!(!f.feed("got message of size 240"));
        assert!(!f.feed(" default 192.168.1.1 default"));
    }

    #[test]
    fn filter_reports_interface_changes() {
        let mut f = RouteFilter::default();
        assert!(f.feed("RTM_IFINFO: iface status change: len 160"));
    }

    #[test]
    fn burst_is_one_event_and_closed_channel_is_an_error() {
        let (tx, rx) = mpsc::channel();
        for _ in 0..5 {
            tx.send(()).unwrap();
        }
        assert_eq!(collect_burst(&rx, ms(20), ms(200)), Ok(()));
        assert!(rx.try_recv().is_err(), "burst fully drained");
        drop(tx);
        assert_eq!(collect_burst(&rx, ms(20), ms(200)), Err(()));
    }
}
