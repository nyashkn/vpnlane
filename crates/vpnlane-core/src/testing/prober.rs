use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use crate::ports::Prober;

/// [`Prober`] with canned latencies; an address not listed gets no reply.
///
/// `Sync` (a `Mutex` instead of a `RefCell`) because probes run concurrently.
#[derive(Debug, Default)]
pub struct FakeProber {
    /// Reply time per address.
    pub latency: HashMap<IpAddr, Duration>,
    /// Addresses probed, in arrival order.
    pub probed: Mutex<Vec<IpAddr>>,
}

impl FakeProber {
    /// Replies from the given `(address, milliseconds)` pairs.
    ///
    /// # Panics
    /// If an address does not parse.
    pub fn with_ms(pairs: &[(&str, u64)]) -> Self {
        Self {
            latency: pairs
                .iter()
                .map(|(ip, ms)| (ip.parse().expect("test ip"), Duration::from_millis(*ms)))
                .collect(),
            probed: Mutex::default(),
        }
    }
}

impl Prober for FakeProber {
    fn probe(&self, ip: IpAddr, _timeout: Duration) -> Option<Duration> {
        self.probed.lock().expect("fake prober lock").push(ip);
        self.latency.get(&ip).copied()
    }
}
