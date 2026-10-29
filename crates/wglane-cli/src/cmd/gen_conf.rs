//! `wglane gen <CC>...`: write configs without connecting.

use color_eyre::Result;
use wglane_core::CountryCode;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Write tunnel configs without connecting",
    long_about = "\
Fetch your NordLynx key and the best server for each COUNTRY and write
<config-dir>/<cc>.conf (mode 600). `up` does this automatically; use `gen`
to prepare configs ahead of time or to force a refresh.",
    after_help = "\
Examples:
  wglane gen ZA US DE
  wglane gen us --config-dir /tmp/wg"
)]
pub(crate) struct Args {
    /// One or more two-letter country codes.
    #[arg(required = true, num_args = 1..)]
    pub(crate) countries: Vec<CountryCode>,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-gen: per country generate_config -> render_wg_conf ->
    // conf_store.write; print "ZA za0012.nordvpn.com load 9% -> ~/.config/wglane/za.conf".
    // Fetch the private key once, not per country.
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane gen").into())
}
