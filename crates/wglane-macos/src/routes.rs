use std::net::IpAddr;

use wglane_core::ports::RouteBypass;
use wglane_core::{BypassSet, Error, Result};

use crate::CommandRunner;

/// [`RouteBypass`] via `/sbin/route`. Needs root, so production use is
/// inside `wglane-helper`.
#[derive(Debug)]
pub struct RouteTable<R> {
    runner: R,
}

impl<R: CommandRunner> RouteTable<R> {
    /// Uses `runner` for every `route`/`netstat` call.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: CommandRunner> RouteBypass for RouteTable<R> {
    fn physical_gateway(&self) -> Result<IpAddr> {
        // TODO(bead): macos-routes-gateway: parse `netstat -rn -f inet` and take
        // the `default` row whose Netif is not utun*; `route -n get default` is
        // wrong once the VPN is up. Table-driven tests with recorded output.
        let _ = &self.runner;
        Err(Error::NotImplemented("RouteTable::physical_gateway"))
    }

    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize> {
        // TODO(bead): macos-routes-apply: per entry `route -n delete` (ignore
        // failure) then `route -n add -host <ip> <gw>` (or `-net <cidr>`); count adds.
        let _ = (set, gateway);
        Err(Error::NotImplemented("RouteTable::apply"))
    }

    fn clear(&self, set: &BypassSet) -> Result<()> {
        // TODO(bead): macos-routes-clear: `route -n delete -host|-net` per entry, ignore "not in table".
        let _ = set;
        Err(Error::NotImplemented("RouteTable::clear"))
    }
}
