use std::cell::RefCell;
use std::net::IpAddr;

use crate::ports::RouteBypass;
use crate::{BypassSet, Result};

/// [`RouteBypass`] that records the installed routes.
#[derive(Debug)]
pub struct FakeRoutes {
    /// Returned by `physical_gateway`.
    pub gateway: IpAddr,
    /// Routes currently "installed", with their gateway.
    pub installed: RefCell<Vec<(ipnet::IpNet, IpAddr)>>,
}

impl FakeRoutes {
    /// No routes, gateway `gateway`.
    pub fn new(gateway: IpAddr) -> Self {
        Self {
            gateway,
            installed: RefCell::default(),
        }
    }
}

impl RouteBypass for FakeRoutes {
    fn physical_gateway(&self) -> Result<IpAddr> {
        Ok(self.gateway)
    }

    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize> {
        let mut installed = self.installed.borrow_mut();
        installed.retain(|(net, _)| !set.iter().any(|n| n == net));
        installed.extend(set.iter().map(|n| (*n, gateway)));
        Ok(set.len())
    }

    fn clear(&self, set: &BypassSet) -> Result<()> {
        self.installed
            .borrow_mut()
            .retain(|(net, _)| !set.iter().any(|n| n == net));
        Ok(())
    }
}
