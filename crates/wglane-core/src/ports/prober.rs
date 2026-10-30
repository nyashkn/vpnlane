use std::net::IpAddr;
use std::time::Duration;

/// Measures the round trip to one VPN server.
pub trait Prober {
    /// Round-trip time to `ip`, or `None` when no reply came within `timeout`
    /// (or the probe could not run). Never an error: for ranking, "no answer"
    /// is the whole story.
    ///
    /// The adapter decides which path the probe takes. While a tunnel is up it
    /// must not travel through it, or it measures the current exit, not `ip`.
    fn probe(&self, ip: IpAddr, timeout: Duration) -> Option<Duration>;
}
