//! `wglane down`: disconnect and remove bypass routes.

use std::path::Path;

use color_eyre::Result;
use wglane_core::ports::Tunnel;
use wglane_macos::{HelperClient, SystemRunner};

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
    let _ = args;
    println!("{}", disconnect_real(ctx)?.summary());
    Ok(())
}

/// What `disconnect` did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Disconnected {
    /// Tunnels that were up and are now down.
    tunnels: usize,
}

impl Disconnected {
    pub(crate) fn summary(&self) -> &'static str {
        if self.tunnels == 0 {
            "nothing to do"
        } else {
            "disconnected"
        }
    }
}

/// `down` against the real adapters. Prints nothing, so the TUI can call it.
pub(crate) fn disconnect_real(ctx: &Context) -> Result<Disconnected> {
    disconnect_recorded(ctx, &HelperClient::new(SystemRunner))
}

/// `disconnect` plus clearing the route-count and active-preset records.
pub(crate) fn disconnect_recorded<T: Tunnel>(ctx: &Context, tunnel: &T) -> Result<Disconnected> {
    let done = disconnect(tunnel, ctx.conf_store().dir())?;
    ctx.clear_routes_record()?;
    ctx.record_preset(None)?;
    Ok(done)
}

/// Brings down every active tunnel; the helper's `down` also removes the bypass routes.
pub(crate) fn disconnect<T: Tunnel>(tunnel: &T, conf_dir: &Path) -> Result<Disconnected> {
    let active = tunnel.active()?;
    for name in &active {
        tunnel.down(&conf_dir.join(format!("{name}.conf")))?;
    }
    Ok(Disconnected {
        tunnels: active.len(),
    })
}

#[cfg(test)]
mod tests {
    use wglane_core::testing::FakeTunnel;

    use super::*;

    #[test]
    fn downs_every_tunnel() {
        let t = FakeTunnel::default();
        t.up.borrow_mut().push("za".into());
        assert_eq!(
            disconnect(&t, Path::new("/cfg")).unwrap().summary(),
            "disconnected"
        );
        assert!(t.up.borrow().is_empty());
    }

    #[test]
    fn nothing_up_is_not_an_error() {
        let t = FakeTunnel::default();
        assert_eq!(
            disconnect(&t, Path::new("/cfg")).unwrap().summary(),
            "nothing to do"
        );
    }
}
