use std::collections::BTreeSet;

use ipnet::IpNet;
use serde::Serialize;

/// Destinations that must skip the VPN and use the physical gateway.
///
/// For Tailscale that is the control server plus every DERP relay; users can
/// add their own hosts and CIDRs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BypassSet(BTreeSet<IpNet>);

impl BypassSet {
    /// Empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one destination. Host addresses are `/32` or `/128`.
    pub fn insert(&mut self, net: IpNet) -> bool {
        self.0.insert(net)
    }

    /// Adds everything from `other`.
    pub fn merge(&mut self, other: BypassSet) {
        self.0.extend(other.0);
    }

    /// Number of destinations.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when there is nothing to bypass.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Destinations in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &IpNet> {
        self.0.iter()
    }
}

impl FromIterator<IpNet> for BypassSet {
    fn from_iter<I: IntoIterator<Item = IpNet>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}
