use std::net::IpAddr;

use crate::{BypassSet, Result};

/// Installs routes that send a [`BypassSet`] via the physical gateway,
/// around the VPN.
pub trait RouteBypass {
    /// Default gateway of the physical interface (not the tunnel). Must be
    /// read before `Tunnel::up` or by ignoring `utun*` routes.
    fn physical_gateway(&self) -> Result<IpAddr>;

    /// Adds a route per destination via `gateway`. Returns how many were added.
    /// Idempotent: existing routes are replaced, not duplicated.
    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize>;

    /// Removes the routes for `set`. Missing routes are not an error.
    fn clear(&self, set: &BypassSet) -> Result<()>;
}
