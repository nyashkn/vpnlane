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
    let helper = HelperClient::new(SystemRunner);
    println!("{}", disconnect(&helper, ctx.conf_store().dir())?);
    Ok(())
}

/// Brings down every active tunnel; the helper's `down` also removes the bypass routes.
fn disconnect<T: Tunnel>(tunnel: &T, conf_dir: &Path) -> Result<&'static str> {
    let active = tunnel.active()?;
    for name in &active {
        tunnel.down(&conf_dir.join(format!("{name}.conf")))?;
    }
    Ok(if active.is_empty() {
        "nothing to do"
    } else {
        "disconnected"
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
        assert_eq!(disconnect(&t, Path::new("/cfg")).unwrap(), "disconnected");
        assert!(t.up.borrow().is_empty());
    }

    #[test]
    fn nothing_up_is_not_an_error() {
        let t = FakeTunnel::default();
        assert_eq!(disconnect(&t, Path::new("/cfg")).unwrap(), "nothing to do");
    }
}
