use std::net::{SocketAddr, UdpSocket};

use wglane_core::ports::UdpProbe;
use wglane_core::{Error, Result};

/// Length of a WireGuard handshake initiation message.
const INITIATION_LEN: usize = 148;

/// [`UdpProbe`] over a plain `std` UDP socket. Needs no privileges.
#[derive(Clone, Copy, Debug, Default)]
pub struct UdpSend;

/// Initiation-shaped: type 1, three reserved zero bytes, zero-filled body. It
/// carries no key material, so the server drops it (its MACs cannot verify).
fn initiation_shaped() -> [u8; INITIATION_LEN] {
    let mut p = [0u8; INITIATION_LEN];
    p[0] = 1;
    p
}

impl UdpProbe for UdpSend {
    fn send_probe(&self, endpoint: SocketAddr) -> Result<()> {
        let any = if endpoint.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let io = |what: &str| {
            let ctx = format!("{what} {endpoint}");
            move |e: std::io::Error| Error::io(format!("{ctx}: {e}"), e)
        };
        let sock = UdpSocket::bind(any).map_err(io("binding a UDP socket for"))?;
        sock.connect(endpoint).map_err(io("routing UDP to"))?;
        sock.send(&initiation_shaped())
            .map_err(io("sending UDP to"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_has_initiation_header_and_length() {
        let p = initiation_shaped();
        assert_eq!((p.len(), p[..4].to_vec()), (148, vec![1, 0, 0, 0]));
    }

    #[test]
    fn delivers_to_a_local_listener() {
        let l = UdpSocket::bind("127.0.0.1:0").expect("bind");
        UdpSend
            .send_probe(l.local_addr().expect("addr"))
            .expect("send");
        let mut buf = [0u8; 200];
        let n = l.recv(&mut buf).expect("recv");
        assert_eq!(n, INITIATION_LEN);
    }
}
