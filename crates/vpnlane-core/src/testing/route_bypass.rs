use std::cell::{Cell, RefCell};
use std::net::IpAddr;
use std::rc::Rc;

use crate::ports::RouteBypass;
use crate::{BypassSet, Error, Result};

/// [`RouteBypass`] that records the installed routes.
#[derive(Debug)]
pub struct FakeRoutes {
    /// Returned by `physical_gateway`; shared so a test can change it mid-flow.
    pub gateway: Rc<Cell<IpAddr>>,
    /// Routes currently "installed", with their gateway.
    pub installed: RefCell<Vec<(ipnet::IpNet, IpAddr)>>,
    /// How many `apply` calls were made, failed ones included.
    pub apply_calls: Cell<usize>,
    /// The next this-many `apply` calls fail without installing anything.
    pub fail_applies: Cell<usize>,
}

impl FakeRoutes {
    /// No routes, gateway `gateway`.
    pub fn new(gateway: IpAddr) -> Self {
        Self {
            gateway: Rc::new(Cell::new(gateway)),
            installed: RefCell::default(),
            apply_calls: Cell::default(),
            fail_applies: Cell::default(),
        }
    }
}

impl RouteBypass for FakeRoutes {
    fn physical_gateway(&self) -> Result<IpAddr> {
        Ok(self.gateway.get())
    }

    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize> {
        self.apply_calls.set(self.apply_calls.get() + 1);
        if self.fail_applies.get() > 0 {
            self.fail_applies.set(self.fail_applies.get() - 1);
            return Err(Error::Api("fake apply failure".into()));
        }
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
