use wglane_core::ports::NetworkWatcher;
use wglane_core::{Error, Result};

/// [`NetworkWatcher`] over `route -n monitor`, which streams routing socket
/// messages: push events, no polling.
#[derive(Debug, Default)]
pub struct RouteMonitor {}

impl RouteMonitor {
    /// Not started until the first `wait_for_change`.
    pub fn new() -> Self {
        Self::default()
    }
}

impl NetworkWatcher for RouteMonitor {
    fn wait_for_change(&mut self) -> Result<()> {
        // TODO(bead): macos-watcher: spawn `route -n monitor` once (piped stdout,
        // kept in self), block reading lines, return on RTM_ADD/RTM_DELETE for
        // `default` or RTM_IFINFO; debounce bursts (~1s) so one Wi-Fi switch is
        // one event. Ignore our own bypass route changes. SCDynamicStore would
        // be cleaner but needs FFI (unsafe); not before v0.2.
        Err(Error::NotImplemented("RouteMonitor::wait_for_change"))
    }
}
