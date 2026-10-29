//! `wglane watch`: re-apply bypass routes on every network change.

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Keep bypass routes correct across network changes",
    long_about = "\
Runs in the foreground. On each network change (Wi-Fi switch, new
gateway) it re-reads the physical gateway and the bypass list and
re-installs the routes, so the tailnet survives roaming. Event-driven,
not polling.",
    after_help = "\
Examples:
  wglane watch
  wglane -v watch      log every change"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-watch: loop { RouteMonitor.wait_for_change()?; if a tunnel is
    // active: gw = physical_gateway; collect bypass; HelperClient.apply }. Log each
    // re-apply at info. Test the loop body with FakeWatcher/FakeRoutes.
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane watch").into())
}
