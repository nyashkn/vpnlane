use std::path::Path;

use wglane_core::{Error, Result};

/// sudoers drop-in written by `install-helper`.
pub const SUDOERS_PATH: &str = "/etc/sudoers.d/wglane";

/// The sudoers line granting `user` passwordless use of the helper only.
/// No wildcards: the helper validates its own arguments.
pub fn render_sudoers_rule(user: &str, helper: &Path) -> Result<String> {
    // TODO(bead): macos-privilege-sudoers: validate `user` ([a-z_][a-z0-9_-]*)
    // and that `helper` is absolute with no whitespace; render
    // "<user> ALL=(root) NOPASSWD: <helper>\n". insta snapshot test.
    let _ = (user, helper);
    Err(Error::NotImplemented("render_sudoers_rule"))
}

/// One-time install, run via `sudo wglane install-helper`: copy the helper to
/// [`crate::HELPER_PATH`] root:wheel 0755, create its root-owned config dir,
/// write [`SUDOERS_PATH`] 0440 after `visudo -cf` validates it.
pub fn install_helper(source_binary: &Path) -> Result<()> {
    // TODO(bead): macos-privilege-install: refuse unless euid 0 (check via
    // `id -u` through CommandRunner, keeping unsafe forbidden); copy, chown,
    // chmod; write sudoers to a temp file, `visudo -cf`, then rename.
    // Needs independent security review before v0.1.
    let _ = source_binary;
    Err(Error::NotImplemented("install_helper"))
}
