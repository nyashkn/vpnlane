//! `wglane status`: active country, exit IP, tailnet check, bypass routes.

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Show country, exit IP, tailnet health and bypass routes",
    long_about = "\
Show the active tunnel, the exit IP the internet sees, whether a tailnet
peer answers, and how many bypass routes are installed.

With --json, prints a TunnelStatus object for scripts and status bars.",
    after_help = "\
Examples:
  wglane status
  wglane st --json | jq .exit_ip",
    visible_alias = "st"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-status: build TunnelStatus from WgQuick::active (no sudo),
    // NordApi::exit_ip, TailscalePing::tailnet, route count; ctx.json ->
    // serde_json::to_writer_pretty(stdout), else aligned text. A failing probe is a
    // field value, not a command failure. insta-snapshot the text renderer.
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane status").into())
}
