use std::net::{Ipv4Addr, SocketAddr};

use ipnet::{IpNet, Ipv4Net};

use crate::{PrivateKey, PublicKey, Server};

/// NordLynx always assigns this interface address.
const NORDLYNX_ADDRESS: Ipv4Addr = Ipv4Addr::new(10, 5, 0, 2);
const NORDLYNX_PORT: u16 = 51820;
const KEEPALIVE_SECS: u16 = 25;

/// Everything needed to render one wg-quick config.
///
/// There is deliberately no DNS field: a `DNS =` line makes wg-quick
/// replace the system resolver and breaks Tailscale MagicDNS.
#[derive(Clone, Debug)]
pub struct TunnelConfig {
    /// Our private key.
    pub private_key: PrivateKey,
    /// Interface address.
    pub address: IpNet,
    /// Server public key.
    pub peer_public_key: PublicKey,
    /// Server endpoint.
    pub endpoint: SocketAddr,
    /// Routed through the tunnel.
    pub allowed_ips: Vec<IpNet>,
    /// Keepalive interval in seconds, if any.
    pub persistent_keepalive: Option<u16>,
}

impl TunnelConfig {
    /// Full-tunnel NordLynx config for `server`, matching the proven script.
    pub fn nordlynx(private_key: PrivateKey, server: &Server) -> Self {
        Self {
            private_key,
            address: IpNet::V4(Ipv4Net::from(NORDLYNX_ADDRESS)),
            peer_public_key: server.public_key.clone(),
            endpoint: SocketAddr::new(server.station, NORDLYNX_PORT),
            // 0.0.0.0/0: full tunnel, as the proven script does.
            allowed_ips: vec![IpNet::V4(Ipv4Net::default())],
            persistent_keepalive: Some(KEEPALIVE_SECS),
        }
    }
}
