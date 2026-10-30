use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::{CountryCode, PublicKey};

/// One WireGuard-capable VPN server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    /// e.g. `us9591.nordvpn.com`.
    pub hostname: String,
    /// Public IP the tunnel endpoint listens on.
    pub station: IpAddr,
    /// Current load, 0-100.
    pub load: u8,
    /// Peer public key for the `[Peer]` section.
    pub public_key: PublicKey,
    /// Country the server is in.
    pub country: CountryCode,
    /// City name, when the provider reports one.
    #[serde(default)]
    pub city: Option<String>,
}
