use std::path::Path;

use wglane_core::ports::Tunnel;
use wglane_core::{Error, Result};

use crate::CommandRunner;

/// Where wg-quick on macOS records active tunnels (`<name>.name` -> `utunN`).
const WG_RUN_DIR: &str = "/var/run/wireguard";

/// [`Tunnel`] via `wg-quick` (Homebrew `wireguard-tools`). Must run as root,
/// so in production it is used only inside `wglane-helper`.
#[derive(Debug)]
pub struct WgQuick<R> {
    runner: R,
}

impl<R: CommandRunner> WgQuick<R> {
    /// Uses `runner` for every `wg-quick` call.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: CommandRunner> Tunnel for WgQuick<R> {
    fn up(&self, conf: &Path) -> Result<()> {
        // TODO(bead): macos-wgquick-up: runner.run_ok("wg-quick", ["up", conf]).
        // Assert via FakeRunner that the exact argv is used and no DNS flag is added.
        let _ = (&self.runner, conf);
        Err(Error::NotImplemented("WgQuick::up"))
    }

    fn down(&self, conf: &Path) -> Result<()> {
        // TODO(bead): macos-wgquick-down: run_ok("wg-quick", ["down", conf]);
        // treat "is not a WireGuard interface" stderr as already-down (Ok).
        let _ = conf;
        Err(Error::NotImplemented("WgQuick::down"))
    }

    fn active(&self) -> Result<Vec<String>> {
        // TODO(bead): macos-wgquick-active: list WG_RUN_DIR/*.name file stems
        // (take the dir as a field so tests can point at a tempdir).
        let _ = WG_RUN_DIR;
        Err(Error::NotImplemented("WgQuick::active"))
    }
}
