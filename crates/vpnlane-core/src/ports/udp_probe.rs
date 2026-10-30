use std::net::SocketAddr;

use crate::Result;

/// Sends one UDP datagram toward a tunnel endpoint without a tunnel.
pub trait UdpProbe {
    /// Sends a WireGuard handshake-initiation-shaped datagram to `endpoint`.
    ///
    /// `Ok` means only that this host let the packet out. A WireGuard server
    /// silently drops initiations it cannot authenticate, so no reply is
    /// expected and a network that drops UDP in transit looks identical.
    /// `Err` means the local stack refused (no route, firewall, interface down).
    fn send_probe(&self, endpoint: SocketAddr) -> Result<()>;
}
