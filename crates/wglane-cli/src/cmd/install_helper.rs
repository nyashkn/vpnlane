//! `wglane install-helper`: one-time setup for passwordless up/down.

use std::path::{Path, PathBuf};

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "One-time setup: install the root helper for passwordless switching",
    long_about = "\
Run once with sudo. Copies wglane-helper to a root-owned path
(/usr/local/libexec/wglane-helper), copies wg and wireguard-go from
Homebrew into root-owned /usr/local/libexec/wglane/bin, refusing any that
load libraries outside /usr/lib and /System/Library (re-run after upgrading
wireguard-tools or wireguard-go), and adds a sudoers rule allowing
only the helper. Afterwards `up`, `down` and `watch` never prompt for a
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

pub(crate) fn run(_ctx: &Context, args: &Args) -> Result<()> {
    if args.uninstall {
        wglane_macos::uninstall_helper()?;
        println!(
            "removed {} and {}",
            wglane_macos::HELPER_PATH,
            wglane_macos::SUDOERS_PATH
        );
        return Ok(());
    }
    let user = std::env::var("SUDO_USER").map_err(|_| {
        eyre!("SUDO_USER is not set: run `sudo wglane install-helper` from your own account")
    })?;
    let source = helper_next_to(&std::env::current_exe()?)?;
    wglane_macos::install_helper(&wglane_macos::SystemRunner, &user, &source)?;
    println!("installed {} for user {user}", wglane_macos::HELPER_PATH);
    Ok(())
}

/// `wglane-helper` in the same directory as the running `wglane` binary.
fn helper_next_to(exe: &Path) -> Result<PathBuf> {
    let helper = exe.with_file_name("wglane-helper");
    if helper.is_file() {
        Ok(helper)
    } else {
        Err(eyre!(
            "{} not found: reinstall wglane so both binaries are together",
            helper.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sibling_helper_or_explains() {
        let d = std::env::temp_dir().join(format!("wglane-ih-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join("wglane");
        assert!(
            helper_next_to(&exe)
                .unwrap_err()
                .to_string()
                .contains("reinstall")
        );
        std::fs::write(d.join("wglane-helper"), b"x").unwrap();
        assert_eq!(helper_next_to(&exe).unwrap(), d.join("wglane-helper"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
