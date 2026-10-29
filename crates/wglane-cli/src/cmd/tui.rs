//! `wglane tui`: full-screen dashboard (cargo feature `tui`).

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Full-screen dashboard: servers, connect, live stats",
    long_about = "\
Full-screen terminal dashboard. Press q to quit.

Built only with the `tui` cargo feature (on by default).",
    after_help = "\
Examples:
  wglane tui"
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let _ = (ctx, args);
    #[cfg(feature = "tui")]
    return crate::tui::run();
    #[cfg(not(feature = "tui"))]
    color_eyre::eyre::bail!("this wglane was built without the `tui` feature");
}
