use wglane_core::ports::BypassProvider;
use wglane_core::{BypassSet, Error, Result};

use crate::CommandRunner;

/// Tailscale/Headscale underlay: the control server and every DERP relay.
///
/// Without host routes for these the tailnet dies under a full tunnel
/// (netcheck reports UDP false), even though 100.64.0.0/10 still routes.
#[derive(Debug)]
pub struct TailscaleBypass<R> {
    runner: R,
}

impl<R: CommandRunner> TailscaleBypass<R> {
    /// Uses `runner` to call the `tailscale` CLI.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: CommandRunner> BypassProvider for TailscaleBypass<R> {
    fn collect(&self) -> Result<BypassSet> {
        // TODO(bead): macos-bypass-tailscale: `tailscale debug prefs` ->
        // .ControlURL host -> resolve (std ToSocketAddrs, IPv4+IPv6) as /32,/128;
        // `tailscale debug derp-map` -> .Regions[].Nodes[].IPv4/.IPv6. Partial
        // failure: return what was found and keep going (the script did). Missing
        // tailscale binary -> empty set. Recorded JSON goes in tests, not fixtures/.
        let _ = &self.runner;
        Err(Error::NotImplemented("TailscaleBypass::collect"))
    }
}

/// User-listed hosts and CIDRs from config, so wglane is not Tailscale-only.
#[derive(Clone, Debug, Default)]
pub struct StaticBypass(pub BypassSet);

impl BypassProvider for StaticBypass {
    fn collect(&self) -> Result<BypassSet> {
        Ok(self.0.clone())
    }
}
