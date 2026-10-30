use std::net::IpAddr;
use std::path::Path;

use crate::Result;

/// Brings WireGuard interfaces up and down.
///
/// macOS: `wglane_macos::WgGo` (as root, inside the helper) and
/// `wglane_macos::HelperClient` (unprivileged, calls the helper).
pub trait Tunnel {
    /// Brings up the tunnel described by the wg-quick config at `conf`.
    /// The tunnel's name is the file stem (`us.conf` -> `us`).
    fn up(&self, conf: &Path) -> Result<()>;

    /// Tears down the tunnel brought up from `conf`.
    fn down(&self, conf: &Path) -> Result<()>;

    /// Names of tunnels currently up.
    fn active(&self) -> Result<Vec<String>>;

    /// Re-points each tunnel's endpoint host route at `gateway` after the
    /// physical network changed. Without it the tunnel's own packets keep
    /// going to the old gateway and every VPN-bound connection stalls.
    fn refresh_endpoint(&self, _gateway: IpAddr) -> Result<()> {
        Ok(())
    }
}
