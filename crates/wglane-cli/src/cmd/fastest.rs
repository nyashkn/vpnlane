//! Concurrent latency probing shared by `up --fastest`, `servers --probe` and
//! the dashboard. Not a subcommand.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use vpnlane_core::ports::{Prober, Tunnel};
use vpnlane_core::{Choice, PROBE_TIMEOUT, Result, Server, pick_fastest};
use vpnlane_macos::{PingProbe, SystemRunner, physical_interface};

/// Probes every server at once. `on` runs on the calling thread as each result
/// arrives (the dashboard streams them); the returned list is in `servers` order.
pub(crate) fn probe_all<P: Prober + Sync + ?Sized>(
    prober: &P,
    servers: &[Server],
    mut on: impl FnMut(usize, Option<Duration>),
) -> Vec<Option<Duration>> {
    let mut out = vec![None; servers.len()];
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        for (i, server) in servers.iter().enumerate() {
            let tx = tx.clone();
            scope.spawn(move || {
                let _ = tx.send((i, prober.probe(server.station, PROBE_TIMEOUT)));
            });
        }
        drop(tx);
        // Ends when every thread has sent and dropped its sender.
        for (i, latency) in rx {
            out[i] = latency;
            on(i, latency);
        }
    });
    out
}

/// The fastest of `candidates` (least loaded first), measured now.
pub(crate) fn choose<P: Prober + Sync + ?Sized>(prober: &P, candidates: Vec<Server>) -> Choice {
    let latencies = probe_all(prober, &candidates, |_, _| {});
    pick_fastest(candidates, &latencies).expect("generate_config passes a non-empty list")
}

/// The interface probes must bind to: the physical one while one of our
/// tunnels is up (else probes would ride the tunnel and measure its exit),
/// none when down.
pub(crate) fn probe_scope<T: Tunnel>(
    tunnel: &T,
    find_physical: impl FnOnce() -> Result<String>,
) -> Result<Option<String>> {
    if tunnel.active()?.is_empty() {
        Ok(None)
    } else {
        find_physical().map(Some)
    }
}

/// A [`PingProbe`] scoped for the current tunnel state.
pub(crate) fn real_prober<T: Tunnel>(tunnel: &T) -> Result<PingProbe<SystemRunner>> {
    let iface = probe_scope(tunnel, || physical_interface(&SystemRunner))?;
    if let Some(iface) = &iface {
        tracing::info!(%iface, "a tunnel is up: probes are bound to the physical interface");
    }
    Ok(PingProbe::new(SystemRunner, iface))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use vpnlane_core::PublicKey;
    use vpnlane_core::testing::{FakeProber, FakeTunnel};

    use super::*;

    fn srv(host: &str, load: u8, ip: &str) -> Server {
        Server {
            hostname: host.into(),
            station: ip.parse().unwrap(),
            load,
            public_key: PublicKey(String::new()),
            country: "ZA".parse().unwrap(),
            city: None,
        }
    }

    fn three() -> Vec<Server> {
        vec![
            srv("quiet", 5, "203.0.113.1"),
            srv("near", 30, "203.0.113.2"),
            srv("dead", 40, "203.0.113.3"),
        ]
    }

    #[test]
    fn probes_every_candidate_and_reports_in_input_order() {
        let p = FakeProber::with_ms(&[("203.0.113.1", 90), ("203.0.113.2", 20)]);
        let mut seen = Vec::new();
        let got = probe_all(&p, &three(), |i, l| seen.push((i, l)));
        let ms = Duration::from_millis;
        assert_eq!(got, [Some(ms(90)), Some(ms(20)), None]);
        seen.sort_unstable();
        assert_eq!(seen.len(), 3, "each result streamed once");
        assert_eq!(p.probed.lock().unwrap().len(), 3);
    }

    #[test]
    fn choose_takes_the_lowest_latency_not_the_lowest_load() {
        let p = FakeProber::with_ms(&[("203.0.113.1", 90), ("203.0.113.2", 20)]);
        let c = choose(&p, three());
        assert_eq!(c.server.hostname, "near");
        assert!(!c.fell_back);
        let c = choose(&FakeProber::default(), three());
        assert_eq!((c.server.hostname.as_str(), c.fell_back), ("quiet", true));
    }

    /// The correctness point: with a tunnel up, probes must be pinned to the
    /// physical interface; with none, they must not be.
    #[test]
    fn scope_binds_only_while_a_tunnel_is_up() {
        let asked = Cell::new(0);
        let find = || {
            asked.set(asked.get() + 1);
            Ok("en0".to_owned())
        };
        let down = FakeTunnel::default();
        assert_eq!(probe_scope(&down, find).unwrap(), None);
        assert_eq!(asked.get(), 0, "no interface lookup when down");

        let up = FakeTunnel::default();
        up.up.borrow_mut().push("us".into());
        assert_eq!(probe_scope(&up, find).unwrap().as_deref(), Some("en0"));
        // No physical interface while up: fail, never probe unbound.
        assert!(probe_scope(&up, || Err(vpnlane_core::Error::NotImplemented("x"))).is_err());
    }
}
