//! `vpnlane install-helper`: one-time setup for passwordless up/down.

use std::path::{Path, PathBuf};

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "One-time setup: install the root helper for passwordless switching",
    long_about = concat!("\
Run once with sudo. Copies ", brand_name!(), "-helper to a root-owned path
(/usr/local/libexec/", brand_name!(), "-helper), copies wg and wireguard-go from
Homebrew into root-owned /usr/local/libexec/", brand_name!(), "/bin, refusing any that
load libraries outside /usr/lib and /System/Library (re-run after upgrading
wireguard-tools or wireguard-go), and adds a sudoers rule allowing
only the helper. Afterwards `up`, `down` and `watch` never prompt for a
password. The helper accepts only fixed verbs with validated arguments."),
    after_help = concat!("\
Examples:
  sudo ", brand_name!(), " install-helper
  sudo ", brand_name!(), " install-helper --uninstall")
)]
pub(crate) struct Args {
    /// Remove the helper and its sudoers rule instead.
    #[arg(long)]
    pub(crate) uninstall: bool,
}

pub(crate) fn run(_ctx: &Context, args: &Args) -> Result<()> {
    if args.uninstall {
        let user = std::env::var("SUDO_USER").ok();
        return report(&vpnlane_macos::uninstall_helper(user.as_deref())?);
    }
    let user = std::env::var("SUDO_USER").map_err(|_| {
        eyre!(concat!(
            "SUDO_USER is not set: run `sudo ",
            brand_name!(),
            " install-helper` from your own account"
        ))
    })?;
    let source = helper_next_to(&std::env::current_exe()?)?;
    report(&vpnlane_macos::install_helper(
        &vpnlane_macos::SystemRunner,
        &user,
        &source,
    )?)
}

/// Prints each step; fails (non-zero exit) if an old file was refused or could not be removed.
fn report(r: &vpnlane_macos::InstallReport) -> Result<()> {
    for s in &r.steps {
        println!("{s}");
    }
    if r.ok() {
        Ok(())
    } else {
        Err(eyre!(
            "some old files were refused or could not be removed (the new install, if any, is in place); see the lines above and docs/UPGRADING.md"
        ))
    }
}

/// `vpnlane-helper` in the same directory as the running `vpnlane` binary.
fn helper_next_to(exe: &Path) -> Result<PathBuf> {
    let helper = exe.with_file_name(concat!(brand_name!(), "-helper"));
    if helper.is_file() {
        Ok(helper)
    } else {
        Err(eyre!(
            concat!(
                "{} not found: reinstall ",
                brand_name!(),
                " so both binaries are together"
            ),
            helper.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sibling_helper_or_explains() {
        let d = std::env::temp_dir().join(format!("vpnlane-ih-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join("vpnlane");
        assert!(
            helper_next_to(&exe)
                .unwrap_err()
                .to_string()
                .contains("reinstall")
        );
        std::fs::write(d.join("vpnlane-helper"), b"x").unwrap();
        assert_eq!(helper_next_to(&exe).unwrap(), d.join("vpnlane-helper"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
