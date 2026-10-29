use std::path::{Path, PathBuf};
use std::time::Duration;

use wglane_core::{CountryCode, Error, Result};

/// Generated wg-quick configs, one per country: `<dir>/<cc>.conf`, mode 0600.
#[derive(Clone, Debug)]
pub struct ConfStore {
    dir: PathBuf,
}

impl ConfStore {
    /// Store rooted at `dir` (normally the config dir).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Where the config for `cc` lives (`us.conf`).
    pub fn path(&self, cc: CountryCode) -> PathBuf {
        self.dir.join(format!("{}.conf", cc.to_lowercase()))
    }

    /// Directory holding the configs.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Age of the config for `cc`; `None` if missing. Feed to `wglane_core::is_stale`.
    pub fn age(&self, cc: CountryCode) -> Result<Option<Duration>> {
        // TODO(bead): macos-conf-store-age: metadata(path).modified(); NotFound -> None;
        // clock skew (modified in the future) -> Some(ZERO). Test with tempfile.
        let _ = cc;
        Err(Error::NotImplemented("ConfStore::age"))
    }

    /// Writes `body` for `cc` atomically with mode 0600, creating the dir 0700.
    pub fn write(&self, cc: CountryCode, body: &str) -> Result<PathBuf> {
        // TODO(bead): macos-conf-store-write: create_dir_all + set 0700; write to
        // `<cc>.conf.tmp` opened with OpenOptions::mode(0o600) + create_new, fsync,
        // rename over target. Test mode bits and that a stale .tmp is handled.
        let _ = (cc, body);
        Err(Error::NotImplemented("ConfStore::write"))
    }
}
