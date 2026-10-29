use std::net::IpAddr;

use serde::Serialize;

/// Snapshot shown by `wglane status` (and `--json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TunnelStatus {
    /// Names of active wglane tunnels (normally zero or one).
    pub active: Vec<String>,
    /// Public IP the internet sees, if it could be determined.
    pub exit_ip: Option<IpAddr>,
    /// Whether the tailnet still works.
    pub tailnet: TailnetHealth,
    /// Number of bypass routes installed.
    pub bypass_routes: usize,
}

/// Result of probing the tailnet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TailnetHealth {
    /// A peer answered.
    Reachable {
        /// Peer that was pinged.
        peer: String,
        /// Round trip in milliseconds.
        latency_ms: u64,
    },
    /// Tailscale is running but no peer answered.
    Unreachable {
        /// Why, in words.
        reason: String,
    },
    /// No `tailscale` CLI found.
    NotInstalled,
}
