//! `wglane install-helper`: one-time setup for passwordless up/down.

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "One-time setup: install the root helper for passwordless switching",
    long_about = "\
Run once with sudo. Copies wglane-helper to a root-owned path
(/usr/local/libexec/wglane-helper) and adds a sudoers rule allowing only
that binary. Afterwards `up`, `down` and `watch` never prompt for a
password. The helper accepts only fixed verbs with validated arguments.",
    after_help = "\
Examples:
  sudo wglane install-helper
  sudo wglane install-helper --uninstall"
)]
pub(crate) struct Args {
    /// Remove the helper and its sudoers rule instead.
    #[arg(long)]
    pub(crate) uninstall: bool,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-install-helper: locate wglane-helper next to current_exe();
    // wglane_macos::install_helper; render_sudoers_rule for $SUDO_USER; --uninstall
    // removes both. Refuse without root with "run: sudo wglane install-helper".
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane install-helper").into())
}
