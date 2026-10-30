use std::net::IpAddr;

use serde::Serialize;

use crate::CountryCode;

/// Snapshot shown by `vpnlane status` (and `--json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TunnelStatus {
    /// Names of active vpnlane tunnels (normally zero or one).
    pub active: Vec<String>,
    /// Public IP the internet sees, if it could be determined.
    pub exit_ip: Option<IpAddr>,
    /// Country Nord says the exit IP is in, if it said.
    pub exit_country: Option<CountryCode>,
    /// Nord's verdict on whether this connection is protected; `None` when the
    /// lookup failed or Nord did not say.
    pub protected: Option<bool>,
    /// Whether the tailnet still works.
    pub tailnet: TailnetHealth,
    /// Number of bypass routes installed.
    pub bypass_routes: usize,
}

/// What Nord's insights endpoint says about our public connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitInsights {
    /// Public IP the internet sees.
    pub ip: IpAddr,
    /// Country of that IP, if Nord gave a valid code.
    pub country: Option<CountryCode>,
    /// Nord's `protected` flag, if present.
    pub protected: Option<bool>,
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
