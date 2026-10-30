use crate::{ExitInsights, Result, TailnetHealth};

/// Asks the provider how it sees our public connection. Sends no credentials.
pub trait ExitIpProbe {
    /// Our public IP, its country and the provider's `protected` verdict.
    /// Fails on timeout or when offline; callers treat that as "unknown".
    fn exit(&self) -> Result<ExitInsights>;
}

/// Checks the tailnet still works with the VPN up.
pub trait TailnetProbe {
    /// Pings a tailnet peer. Not having Tailscale installed is a health
    /// state, not an error.
    fn tailnet(&self) -> Result<TailnetHealth>;
}
