//! `wglane doctor`: check prerequisites and print the fix for each failure.

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Check prerequisites and print how to fix each problem",
    long_about = "\
Checks: wg-quick installed, tailscale installed and logged in, helper
installed root-owned with a valid sudoers rule, token present with mode
600, Nord API reachable and its responses still parse.

Every failed check prints the exact command to fix it.",
    after_help = "\
Examples:
  wglane doctor
  wglane doctor --json"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-doctor: one check fn per row returning Pass | Fail{fix}; exit
    // non-zero if any fail. e.g. fix "brew install wireguard-tools",
    // "sudo wglane install-helper", "chmod 600 ~/.config/wglane/token".
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane doctor").into())
}
