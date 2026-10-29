use crate::Result;

/// Push notifications for network changes (Wi-Fi switch, new gateway).
///
/// Must block on a system event source, not poll.
pub trait NetworkWatcher {
    /// Blocks until the routing table or primary interface changes.
    fn wait_for_change(&mut self) -> Result<()>;
}
