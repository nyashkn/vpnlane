//! `wglane up <CC>`: connect, regenerating the config if missing or stale.

use color_eyre::Result;
use wglane_core::CountryCode;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Connect to a country (switches if already connected)",
    long_about = "\
Connect to the least-loaded NordLynx server in COUNTRY.

If the config for COUNTRY is missing or older than 7 days it is regenerated
first (no separate `gen` step). An existing wglane tunnel is brought down.

Why Tailscale breaks, and what wglane does:
  A full tunnel (AllowedIPs 0.0.0.0/0) also captures Tailscale's underlay:
  the control server and DERP relays. netcheck then reports UDP false and the
  tailnet dies, although 100.64.0.0/10 still routes. wglane adds host routes
  for those IPs via your physical gateway, so the tailnet keeps working and
  everything else exits through the VPN. The config has no DNS= line, so
  MagicDNS keeps working too.",
    after_help = "\
Examples:
  wglane up ZA          South Africa
  wglane up us          case does not matter
  wglane -v up DE       show each step"
)]
pub(crate) struct Args {
    /// Two-letter country code, e.g. US, DE, ZA (not UK: use GB).
    pub(crate) country: CountryCode,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    // TODO(bead): cli-up: store = ctx.conf_store(); if is_stale(store.age(cc)?) {
    // generate_config(NordApi, ctx.token_source(), cc) -> render_wg_conf -> store.write };
    // gw = HelperClient.physical_gateway() BEFORE tunnel up; HelperClient down (ignore
    // not-up) then up(store.path(cc)); merge TailscaleBypass + StaticBypass; apply(set, gw).
    // Print "connected to ZA via za0012 (load 9%), 23 bypass routes". Test with fakes.
    let _ = (ctx, args);
    Err(wglane_core::Error::NotImplemented("wglane up").into())
}
