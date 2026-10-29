use std::path::Path;

use crate::Result;

/// Brings WireGuard interfaces up and down.
///
/// macOS: `wglane_macos::WgQuick` (as root, inside the helper) and
/// `wglane_macos::HelperClient` (unprivileged, calls the helper).
pub trait Tunnel {
    /// Brings up the tunnel described by the wg-quick config at `conf`.
    /// The tunnel's name is the file stem (`us.conf` -> `us`).
    fn up(&self, conf: &Path) -> Result<()>;

    /// Tears down the tunnel brought up from `conf`.
    fn down(&self, conf: &Path) -> Result<()>;

    /// Names of tunnels currently up.
    fn active(&self) -> Result<Vec<String>>;
}
