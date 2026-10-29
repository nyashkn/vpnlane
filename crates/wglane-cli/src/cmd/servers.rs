//! `wglane servers [CC]`: list countries, or ranked servers in one.

use color_eyre::Result;
use wglane_core::CountryCode;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "List countries, or ranked servers in one country",
    long_about = "\
Without COUNTRY: every country the provider serves.
With COUNTRY: its WireGuard servers, least loaded first (ties keep
Nord's distance order).",
    after_help = "\
Examples:
  wglane servers
  wglane servers ZA
  wglane ls DE --limit 10 --json",
    visible_alias = "ls"
)]
pub(crate) struct Args {
    /// Country to list servers for; omit to list countries.
    pub(crate) country: Option<CountryCode>,

    /// How many servers to show.
    #[arg(long, default_value_t = 5)]
    pub(crate) limit: usize,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-servers: NordApi countries() or recommended(cc, limit) +
    // rank_by_load; table "HOST  LOAD  IP" or --json. No token needed.
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane servers").into())
}
