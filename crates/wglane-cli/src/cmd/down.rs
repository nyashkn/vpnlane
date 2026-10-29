//! `wglane down`: disconnect and remove bypass routes.

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Disconnect and remove bypass routes",
    long_about = "\
Bring down every wglane tunnel and remove the bypass routes it added.
Safe to run when nothing is up.",
    after_help = "\
Examples:
  wglane down"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-down: HelperClient::down via the helper (removes routes too);
    // print "disconnected" or "nothing to do".
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane down").into())
}
