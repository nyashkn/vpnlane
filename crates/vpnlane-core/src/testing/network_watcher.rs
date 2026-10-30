use crate::ports::NetworkWatcher;
use crate::{Error, Result};

/// [`NetworkWatcher`] that reports `changes` events, then fails so loops end.
#[derive(Debug, Default)]
pub struct FakeWatcher {
    /// Events left to deliver.
    pub changes: usize,
}

impl NetworkWatcher for FakeWatcher {
    fn wait_for_change(&mut self) -> Result<()> {
        if self.changes == 0 {
            return Err(Error::io(
                "fake watcher exhausted",
                std::io::ErrorKind::UnexpectedEof.into(),
            ));
        }
        self.changes -= 1;
        Ok(())
    }
}
