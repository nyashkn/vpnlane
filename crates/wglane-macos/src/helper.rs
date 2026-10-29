use std::net::IpAddr;
use std::path::Path;

use wglane_core::ports::{RouteBypass, Tunnel};
use wglane_core::{BypassSet, Error, Result};

use crate::CommandRunner;

/// Root-owned install location of `wglane-helper` (never the user-writable
/// Homebrew copy: a sudoers rule on a user-writable binary is a root exploit).
pub const HELPER_PATH: &str = "/usr/local/libexec/wglane-helper";

/// The complete helper protocol. This verb set becomes the socket protocol
/// if wglane later grows a launchd daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperVerb {
    /// `up <cc>`: wg-quick up the root-owned copy of `<cc>.conf`.
    Up,
    /// `down`: tear down every wglane tunnel and its bypass routes.
    Down,
    /// `bypass <gateway> <cidr>...`: (re)install bypass routes.
    Bypass,
}

impl HelperVerb {
    /// Wire name of the verb.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Bypass => "bypass",
        }
    }
}

/// Unprivileged side: implements [`Tunnel`] and [`RouteBypass`] by running
/// `sudo -n <HELPER_PATH> <verb> <args>`.
#[derive(Debug)]
pub struct HelperClient<R> {
    runner: R,
}

impl<R: CommandRunner> HelperClient<R> {
    /// Uses `runner` to call `sudo`.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: CommandRunner> Tunnel for HelperClient<R> {
    fn up(&self, conf: &Path) -> Result<()> {
        // TODO(bead): macos-helper-client-up: pass only the file stem (cc) — the
        // helper never takes paths from the user. `sudo -n` failure (exit 1,
        // "a password is required") -> Error::Command with hint "run `sudo wglane install-helper`".
        let _ = (&self.runner, conf, HelperVerb::Up);
        Err(Error::NotImplemented("HelperClient::up"))
    }

    fn down(&self, conf: &Path) -> Result<()> {
        // TODO(bead): macos-helper-client-down: `sudo -n HELPER_PATH down`.
        let _ = conf;
        Err(Error::NotImplemented("HelperClient::down"))
    }

    fn active(&self) -> Result<Vec<String>> {
        // TODO(bead): macos-helper-client-active: /var/run/wireguard is world-readable, so
        // delegate to WgQuick::active without sudo.
        Err(Error::NotImplemented("HelperClient::active"))
    }
}

impl<R: CommandRunner> RouteBypass for HelperClient<R> {
    fn physical_gateway(&self) -> Result<IpAddr> {
        // TODO(bead): macos-helper-client-gateway: unprivileged; delegate to RouteTable.
        Err(Error::NotImplemented("HelperClient::physical_gateway"))
    }

    fn apply(&self, set: &BypassSet, gateway: IpAddr) -> Result<usize> {
        // TODO(bead): macos-helper-client-bypass: `sudo -n HELPER_PATH bypass <gw> <cidr>...`.
        let _ = (set, gateway, HelperVerb::Bypass);
        Err(Error::NotImplemented("HelperClient::apply"))
    }

    fn clear(&self, set: &BypassSet) -> Result<()> {
        // TODO(bead): macos-helper-client-clear: covered by `down`; decide whether a
        // separate verb is needed (see to-ask in docs/ARCHITECTURE.md).
        let _ = set;
        Err(Error::NotImplemented("HelperClient::clear"))
    }
}
