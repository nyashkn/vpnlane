use std::path::{Path, PathBuf};

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
    run_dir: PathBuf,
}

impl<R: CommandRunner> WgQuick<R> {
    /// Uses `runner` for every `wg-quick` call.
    pub fn new(runner: R) -> Self {
        Self::with_run_dir(runner, WG_RUN_DIR)
    }

    /// Like [`new`](Self::new) but reads active tunnels from `run_dir` (tests).
    pub fn with_run_dir(runner: R, run_dir: impl Into<PathBuf>) -> Self {
        Self {
            runner,
            run_dir: run_dir.into(),
        }
    }
}

/// wg-quick's own interface-name rule (`[a-zA-Z0-9_=+.-]{1,15}`).
fn valid_interface_name(name: &str) -> bool {
    (1..=15).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_=+.-".contains(&b))
}

/// Checks `conf` is an absolute `<name>.conf` path with a legal interface
/// name and returns it as UTF-8. Runs as root: nothing else reaches argv.
fn checked_conf(conf: &Path) -> Result<&str> {
    let bad = |why: &str| {
        Error::io(
            format!("refusing wg-quick config {}: {why}", conf.display()),
            std::io::Error::from(std::io::ErrorKind::InvalidInput),
        )
    };
    if !conf.is_absolute() {
        return Err(bad("path must be absolute"));
    }
    if conf.extension().and_then(|e| e.to_str()) != Some("conf") {
        return Err(bad("file must end in .conf"));
    }
    if !conf
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(valid_interface_name)
    {
        return Err(bad("name must be 1-15 characters of [A-Za-z0-9_=+.-]"));
    }
    conf.to_str().ok_or_else(|| bad("path is not UTF-8"))
}

impl<R: CommandRunner> Tunnel for WgQuick<R> {
    fn up(&self, conf: &Path) -> Result<()> {
        let conf = checked_conf(conf)?;
        self.runner.run_ok("wg-quick", &["up", conf]).map(drop)
    }

    fn down(&self, conf: &Path) -> Result<()> {
        let conf = checked_conf(conf)?;
        let out = self.runner.run("wg-quick", &["down", conf])?;
        // Already down is the state the caller wanted.
        if out.success() || out.stderr.contains("is not a WireGuard interface") {
            return Ok(());
        }
        Err(Error::Command {
            program: "wg-quick".into(),
            status: out
                .code
                .map_or_else(|| "killed by signal".into(), |c| format!("exit {c}")),
            stderr: out.stderr.trim().to_owned(),
        })
    }

    fn active(&self) -> Result<Vec<String>> {
        let entries = match std::fs::read_dir(&self.run_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(Error::io(format!("reading {}", self.run_dir.display()), e));
            }
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| {
                let p = e.ok()?.path();
                if p.extension()? != "name" {
                    return None;
                }
                p.file_stem()?.to_str().map(str::to_owned)
            })
            .collect();
        names.sort();
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use crate::testing::FakeRunner;

    fn quick() -> WgQuick<FakeRunner> {
        WgQuick::new(FakeRunner::new())
    }

    #[test]
    fn up_runs_exact_argv() {
        let q = quick();
        q.up(Path::new("/var/root/wglane/us.conf")).unwrap();
        assert_eq!(
            q.runner.calls(),
            vec![vec!["wg-quick", "up", "/var/root/wglane/us.conf"]]
        );
    }

    #[test]
    fn up_failure_is_command_error() {
        let q = quick();
        q.runner.push(CommandOutput {
            code: Some(1),
            stderr: "boom\n".into(),
            ..Default::default()
        });
        let err = q.up(Path::new("/c/us.conf")).unwrap_err();
        assert_eq!(err.to_string(), "`wg-quick` failed (exit 1): boom");
    }

    #[test]
    fn rejects_unsafe_conf_paths_without_running_anything() {
        let q = quick();
        for bad in [
            "us.conf",
            "/c/us",
            "/c/us.conf.bak",
            "/c/.conf",
            "/c/a b.conf",
            "/c/us;rm.conf",
            "/c/-x.conf/../u s.conf",
            "/c/averyveryverylongname.conf",
            "/c/ü.conf",
        ] {
            assert!(q.up(Path::new(bad)).is_err(), "{bad}");
            assert!(q.down(Path::new(bad)).is_err(), "{bad}");
        }
        assert_eq!(q.runner.calls().len(), 0);
    }

    #[test]
    fn down_runs_exact_argv() {
        let q = quick();
        q.down(Path::new("/c/us.conf")).unwrap();
        assert_eq!(
            q.runner.calls(),
            vec![vec!["wg-quick", "down", "/c/us.conf"]]
        );
    }

    #[test]
    fn down_when_already_down_is_ok() {
        let q = quick();
        q.runner.push(CommandOutput {
            code: Some(1),
            stderr: "wg-quick: `us' is not a WireGuard interface\n".into(),
            ..Default::default()
        });
        q.down(Path::new("/c/us.conf")).unwrap();
    }

    #[test]
    fn down_other_failure_is_error() {
        let q = quick();
        q.runner.push(CommandOutput {
            code: Some(1),
            stderr: "denied\n".into(),
            ..Default::default()
        });
        let err = q.down(Path::new("/c/us.conf")).unwrap_err();
        assert_eq!(err.to_string(), "`wg-quick` failed (exit 1): denied");
    }

    #[test]
    fn active_lists_name_file_stems() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["us.name", "de.name", "us.sock", "notes"] {
            std::fs::write(dir.path().join(f), "utun4\n").unwrap();
        }
        let q = WgQuick::with_run_dir(FakeRunner::new(), dir.path());
        assert_eq!(q.active().unwrap(), vec!["de", "us"]);
        assert_eq!(q.runner.calls().len(), 0);
    }

    #[test]
    fn active_without_run_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let q = WgQuick::with_run_dir(FakeRunner::new(), dir.path().join("missing"));
        assert_eq!(q.active().unwrap().len(), 0);
    }
}
