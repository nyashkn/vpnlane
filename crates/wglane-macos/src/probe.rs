use wglane_core::ports::TailnetProbe;
use wglane_core::{Error, Result, TailnetHealth};

use crate::CommandRunner;

/// [`TailnetProbe`] via `tailscale ping`.
#[derive(Debug)]
pub struct TailscalePing<R> {
    runner: R,
}

impl<R: CommandRunner> TailscalePing<R> {
    /// Uses `runner` to call the `tailscale` CLI.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: CommandRunner> TailnetProbe for TailscalePing<R> {
    fn tailnet(&self) -> Result<TailnetHealth> {
        // TODO(bead): macos-probe-tailnet: pick an online peer from
        // `tailscale status --json` (.Peer[].Online, .DNSName); run
        // `tailscale ping -c 1 --timeout 3s <peer>`; parse "in 23ms" into
        // Reachable. tailscale missing (Error::Io NotFound) -> NotInstalled.
        let _ = &self.runner;
        Err(Error::NotImplemented("TailscalePing::tailnet"))
    }
}
