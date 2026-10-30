//! `vpnlane tui`: full-screen dashboard (cargo feature `tui`).

use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Full-screen dashboard: servers, connect, live stats",
    long_about = "\
Full-screen terminal dashboard. Press q to quit.

Built only with the `tui` cargo feature (on by default).",
    after_help = concat!("\
Examples:
  ", brand_name!(), " tui")
)]
pub(crate) struct Args {}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    let _ = args;
    #[cfg(feature = "tui")]
    return crate::tui::run(ctx);
    #[cfg(not(feature = "tui"))]
    {
        let _ = ctx;
        color_eyre::eyre::bail!(concat!(
            "this ",
            brand_name!(),
            " was built without the `tui` feature"
        ));
    }
}
