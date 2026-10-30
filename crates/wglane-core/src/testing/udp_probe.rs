use std::cell::RefCell;
use std::net::SocketAddr;

use crate::ports::UdpProbe;
use crate::{Error, Result};

/// [`UdpProbe`] that records endpoints and optionally refuses.
#[derive(Debug, Default)]
pub struct FakeUdp {
    /// Make every send fail with this I/O error kind.
    pub refuse: Option<std::io::ErrorKind>,
    /// Endpoints probed, in order.
    pub sent: RefCell<Vec<SocketAddr>>,
}

impl UdpProbe for FakeUdp {
    fn send_probe(&self, endpoint: SocketAddr) -> Result<()> {
        self.sent.borrow_mut().push(endpoint);
        self.refuse.map_or(Ok(()), |k| {
            Err(Error::io(
                format!("sending UDP to {endpoint}: network unreachable"),
                k.into(),
            ))
        })
    }
}
