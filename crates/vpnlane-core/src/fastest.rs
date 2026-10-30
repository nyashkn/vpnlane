//! Picking a server by measured latency. Pure: probing is the caller's job.

use std::time::Duration;

use crate::Server;

/// How many of the least-loaded servers `--fastest` measures.
// shortcut: fixed, not a setting; make it config when someone asks.
pub const FASTEST_CANDIDATES: usize = 10;

/// How long one probe may take before the server counts as unreachable.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// The server picked and why it can be trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The server to connect to.
    pub server: Server,
    /// Its measured round trip; `None` when it was not (or could not be) probed.
    pub latency: Option<Duration>,
    /// Latency was asked for but every probe failed, so `server` is simply the
    /// least loaded. The caller must say so.
    pub fell_back: bool,
}

impl Choice {
    /// The least loaded of `servers` (first of a load-ranked, non-empty list).
    ///
    /// # Panics
    /// If `servers` is empty; callers check that first.
    pub fn least_load(servers: Vec<Server>) -> Self {
        Self {
            server: servers.into_iter().next().expect("non-empty candidates"),
            latency: None,
            fell_back: false,
        }
    }
}

/// The fastest of `candidates`, given in least-load-first order with
/// `latencies[i]` the probe result of `candidates[i]`.
///
/// Lowest latency wins; ties keep the least-load order; servers without a
/// reply lose to any that replied. If none replied, the first (least loaded)
/// is returned with `fell_back` set. `None` only for an empty list.
pub fn pick_fastest(candidates: Vec<Server>, latencies: &[Option<Duration>]) -> Option<Choice> {
    let best = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, _)| Some((i, (*latencies.get(i)?)?)))
        .min_by_key(|&(i, l)| (l, i));
    let (index, latency) = best.map_or((0, None), |(i, l)| (i, Some(l)));
    let server = candidates.into_iter().nth(index)?;
    Some(Choice {
        server,
        latency,
        fell_back: latency.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PublicKey;

    fn s(name: &str, load: u8) -> Server {
        Server {
            hostname: name.into(),
            station: "192.0.2.1".parse().unwrap(),
            load,
            public_key: PublicKey(String::new()),
            country: "DE".parse().unwrap(),
            city: None,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn lowest_latency_wins_over_lower_load() {
        let c = pick_fastest(
            vec![s("quiet", 5), s("near", 40)],
            &[Some(ms(90)), Some(ms(20))],
        )
        .unwrap();
        assert_eq!(
            (c.server.hostname.as_str(), c.latency, c.fell_back),
            ("near", Some(ms(20)), false)
        );
    }

    #[test]
    fn ties_keep_least_load_order() {
        let c = pick_fastest(vec![s("a", 1), s("b", 2)], &[Some(ms(30)), Some(ms(30))]).unwrap();
        assert_eq!(c.server.hostname, "a");
    }

    #[test]
    fn failed_probes_lose_to_any_reply() {
        let c = pick_fastest(vec![s("a", 1), s("b", 2)], &[None, Some(ms(900))]).unwrap();
        assert_eq!((c.server.hostname.as_str(), c.fell_back), ("b", false));
    }

    #[test]
    fn all_failed_falls_back_to_least_load_and_says_so() {
        let c = pick_fastest(vec![s("a", 1), s("b", 2)], &[None, None]).unwrap();
        assert_eq!(
            (c.server.hostname.as_str(), c.latency, c.fell_back),
            ("a", None, true)
        );
        // A short latency list counts as missing replies, not a panic.
        assert!(pick_fastest(vec![s("a", 1)], &[]).unwrap().fell_back);
        assert!(pick_fastest(vec![], &[]).is_none());
    }
}
