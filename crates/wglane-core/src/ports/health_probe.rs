use std::net::IpAddr;

use crate::{Result, TailnetHealth};

/// Asks the outside world which IP we exit from (provider API).
pub trait ExitIpProbe {
    /// Our public IP as seen by the provider.
    fn exit_ip(&self) -> Result<IpAddr>;
}

/// Checks the tailnet still works with the VPN up.
pub trait TailnetProbe {
    /// Pings a tailnet peer. Not having Tailscale installed is a health
    /// state, not an error.
    fn tailnet(&self) -> Result<TailnetHealth>;
}
