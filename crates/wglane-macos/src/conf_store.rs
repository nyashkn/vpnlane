use std::fs::{self, OpenOptions, Permissions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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
        let path = self.path(cc);
        let modified = match fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::io(format!("reading age of {}", path.display()), e)),
        };
        // Modified in the future (clock skew) counts as brand new.
        Ok(Some(
            SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO),
        ))
    }

    /// Writes `body` for `cc` atomically with mode 0600, creating the dir 0700.
    pub fn write(&self, cc: CountryCode, body: &str) -> Result<PathBuf> {
        let path = self.path(cc);
        let tmp = path.with_extension("conf.tmp");
        let io = |what: &str, p: &Path| {
            let ctx = format!("{what} {}", p.display());
            move |e| Error::io(ctx, e)
        };
        fs::create_dir_all(&self.dir).map_err(io("creating", &self.dir))?;
        fs::set_permissions(&self.dir, Permissions::from_mode(0o700))
            .map_err(io("securing", &self.dir))?;
        // A tmp left by a crashed write may have loose perms; create_new needs it gone.
        match fs::remove_file(&tmp) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(io("removing", &tmp)(e)),
            _ => {}
        }
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(io("creating", &tmp))?;
        f.write_all(body.as_bytes())
            .and_then(|()| f.sync_all())
            .map_err(io("writing", &tmp))?;
        fs::rename(&tmp, &path).map_err(io("replacing", &path))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us() -> CountryCode {
        "US".parse().expect("valid code")
    }

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).expect("metadata").permissions().mode() & 0o777
    }

    #[test]
    fn write_sets_modes_and_replaces() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ConfStore::new(tmp.path().join("wglane"));
        let path = store.write(us(), "one").expect("write");
        assert_eq!(path, store.path(us()));
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(&path), 0o600);
        store.write(us(), "two").expect("rewrite");
        assert_eq!(fs::read_to_string(&path).expect("read"), "two");
    }

    #[test]
    fn write_recovers_from_stale_loose_tmp() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ConfStore::new(tmp.path());
        let stale = tmp.path().join("us.conf.tmp");
        fs::write(&stale, "junk").expect("seed tmp");
        fs::set_permissions(&stale, Permissions::from_mode(0o644)).expect("chmod");
        let path = store.write(us(), "key").expect("write");
        assert_eq!(mode(&path), 0o600);
        assert!(!stale.exists());
    }

    #[test]
    fn age_missing_fresh_old_and_future() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ConfStore::new(tmp.path());
        assert_eq!(store.age(us()).expect("age"), None);
        let path = store.write(us(), "x").expect("write");
        assert!(store.age(us()).expect("age").expect("some") < Duration::from_secs(5));
        let set = |t| {
            fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_modified(t))
                .expect("set mtime");
        };
        set(SystemTime::now() - Duration::from_secs(3600));
        assert!(store.age(us()).expect("age").expect("some") >= Duration::from_secs(3600));
        set(SystemTime::now() + Duration::from_secs(3600));
        assert_eq!(store.age(us()).expect("age"), Some(Duration::ZERO));
    }
}
