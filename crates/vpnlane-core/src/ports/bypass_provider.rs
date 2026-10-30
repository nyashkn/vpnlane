use crate::{BypassSet, Result};

/// Produces destinations that must bypass the VPN.
///
/// Tailscale/Headscale is one provider (control host + DERP map); a user's
/// configured hosts/CIDRs are another. The CLI merges all providers.
pub trait BypassProvider {
    /// Current destinations. Called on `up` and again on every network change.
    fn collect(&self) -> Result<BypassSet>;
}
