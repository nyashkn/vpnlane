use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use wglane_core::{Error, Result};

use crate::{CommandRunner, HELPER_PATH};

/// sudoers drop-in written by `install-helper`.
pub const SUDOERS_PATH: &str = "/etc/sudoers.d/wglane";

const CHOWN: &str = "/usr/sbin/chown";
const VISUDO: &str = "/usr/sbin/visudo";

fn invalid(msg: impl Into<String>) -> Error {
    Error::io(
        msg,
        io::Error::new(ErrorKind::InvalidInput, "invalid input"),
    )
}

/// The sudoers line granting `user` passwordless use of the helper only.
/// No wildcards: the helper validates its own arguments.
///
/// Fails if `user` is not `[a-z_][a-z0-9_-]*` (at most 31 bytes) or `helper`
/// is not a plain absolute path of `[A-Za-z0-9/._+-]` without `..`.
pub fn render_sudoers_rule(user: &str, helper: &Path) -> Result<String> {
    let mut chars = user.chars();
    let head_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    let tail_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !head_ok || !tail_ok || user.len() > 31 {
        return Err(invalid(format!(
            "invalid user name {user:?}: need lowercase letters, digits, `_` or `-`, starting with a letter or `_`"
        )));
    }
    // Strict allowlist: every sudoers metacharacter (space , : = \ # ! ( ) * ? [ " ') is excluded.
    let path = helper.to_str().filter(|p| {
        p.starts_with('/')
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._+-".contains(&b))
            && helper
                .components()
                .all(|c| !matches!(c, Component::ParentDir))
    });
    let path = path.ok_or_else(|| {
        invalid(format!(
            "unsafe helper path {}: need an absolute path of [A-Za-z0-9/._+-] without `..`",
            helper.display()
        ))
    })?;
    Ok(format!("{user} ALL=(root) NOPASSWD: {path}\n"))
}

/// Where [`install_helper`] writes. Production values come from [`Layout::system`];
/// tests point it into a temp dir.
struct Layout {
    helper: PathBuf,
    sudoers: PathBuf,
    /// Uid that must own the helper's directory (0 in production).
    owner_uid: u32,
}

impl Layout {
    fn system() -> Self {
        Self {
            helper: HELPER_PATH.into(),
            sudoers: SUDOERS_PATH.into(),
            owner_uid: 0,
        }
    }
}

fn require_root() -> Result<()> {
    if rustix::process::geteuid().is_root() {
        Ok(())
    } else {
        Err(Error::io(
            "install-helper needs root: run `sudo wglane install-helper`",
            io::Error::from(ErrorKind::PermissionDenied),
        ))
    }
}

/// One-time install, run via `sudo wglane install-helper`: copy the helper to
/// [`crate::HELPER_PATH`] root:wheel 0755 and write [`SUDOERS_PATH`] 0440 for
/// `user` after `visudo -cf` validates it.
///
/// Refuses unless the effective uid is 0. Safe to re-run.
pub fn install_helper<R: CommandRunner>(
    runner: &R,
    user: &str,
    source_binary: &Path,
) -> Result<()> {
    require_root()?;
    install_at(runner, &Layout::system(), user, source_binary)
}

/// Removes the sudoers rule first (so privilege is gone even if the second
/// step fails), then the helper. Missing files are fine. Requires root.
pub fn uninstall_helper() -> Result<()> {
    require_root()?;
    uninstall_at(&Layout::system())
}

fn uninstall_at(l: &Layout) -> Result<()> {
    for p in [&l.sudoers, &l.helper] {
        match fs::remove_file(p) {
            Err(e) if e.kind() != ErrorKind::NotFound => {
                return Err(Error::io(format!("removing {}", p.display()), e));
            }
            _ => {}
        }
    }
    Ok(())
}

fn install_at<R: CommandRunner>(runner: &R, l: &Layout, user: &str, source: &Path) -> Result<()> {
    // Render (and so validate) before touching the system.
    let rule = render_sudoers_rule(user, &l.helper)?;
    let dir = l
        .helper
        .parent()
        .ok_or_else(|| invalid("helper path has no parent"))?;
    fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    let meta =
        fs::symlink_metadata(dir).map_err(|e| Error::io(format!("stat {}", dir.display()), e))?;
    if !meta.is_dir() || meta.uid() != l.owner_uid || meta.mode() & 0o022 != 0 {
        return Err(invalid(format!(
            "{d} must be a real directory owned by root and not group/world-writable; run `sudo chown root:wheel {d} && sudo chmod 755 {d}`",
            d = dir.display()
        )));
    }

    // Helper binary: temp file created O_EXCL (never follows a planted symlink), then rename.
    let tmp = sibling(&l.helper, ".new");
    let mut src =
        File::open(source).map_err(|e| Error::io(format!("opening {}", source.display()), e))?;
    write_new(&tmp, 0o755, |dst| io::copy(&mut src, dst).map(drop))?;
    finish(runner, &tmp, &l.helper)?;

    // Sudoers: the temp name contains `.`, so sudo ignores it until the rename.
    let tmp = sibling(&l.sudoers, ".tmp");
    write_new(&tmp, 0o440, |dst| {
        io::Write::write_all(dst, rule.as_bytes())
    })?;
    let tmp_str = tmp.to_string_lossy();
    if let Err(e) = runner.run_ok(VISUDO, &["-cf", &tmp_str]) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    finish(runner, &tmp, &l.sudoers)
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Creates `path` exclusively with `mode`, filling it with `fill`.
fn write_new(path: &Path, mode: u32, fill: impl FnOnce(&mut File) -> io::Result<()>) -> Result<()> {
    let ctx = |e| Error::io(format!("writing {}", path.display()), e);
    match fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => return Err(ctx(e)),
        _ => {}
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(ctx)?;
    fill(&mut f).map_err(ctx)?;
    // umask may have stripped bits at create time.
    f.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(ctx)?;
    f.sync_all().map_err(ctx)
}

/// chown root:wheel then atomically rename `tmp` over `dst`; removes `tmp` on failure.
fn finish<R: CommandRunner>(runner: &R, tmp: &Path, dst: &Path) -> Result<()> {
    let step = || -> Result<()> {
        runner.run_ok(CHOWN, &["root:wheel", &tmp.to_string_lossy()])?;
        fs::rename(tmp, dst).map_err(|e| Error::io(format!("installing {}", dst.display()), e))
    };
    step().inspect_err(|_| {
        let _ = fs::remove_file(tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandOutput;
    use crate::testing::FakeRunner;

    #[test]
    fn rule_snapshot() {
        let rule = render_sudoers_rule("kn", Path::new(HELPER_PATH)).unwrap();
        insta::assert_snapshot!(rule, @"kn ALL=(root) NOPASSWD: /usr/local/libexec/wglane-helper");
    }

    #[test]
    fn rejects_bad_users() {
        let h = Path::new(HELPER_PATH);
        let long = "a".repeat(32);
        for u in [
            "",
            "Root",
            "1abc",
            "a b",
            "a,b",
            "a\nALL ALL=(ALL) NOPASSWD: ALL",
            "a.b",
            "-x",
            "ünï",
            "a:b",
            &long,
        ] {
            assert!(render_sudoers_rule(u, h).is_err(), "{u:?} accepted");
        }
        for u in ["kn", "_www", "a-b_c9", &long[1..]] {
            assert!(render_sudoers_rule(u, h).is_ok(), "{u:?} rejected");
        }
    }

    #[test]
    fn rejects_bad_helper_paths() {
        for p in [
            "relative/helper",
            "/usr/local/a b",
            "/usr/../etc/x",
            "/x/*",
            "/x,/bin/sh",
            "/x\\ y",
            "/x#",
            "/x=y",
            "",
        ] {
            assert!(
                render_sudoers_rule("kn", Path::new(p)).is_err(),
                "{p:?} accepted"
            );
        }
    }

    fn fixture() -> (tempfile::TempDir, Layout, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("libexec");
        let sudoers_d = t.path().join("sudoers.d");
        fs::create_dir(&sudoers_d).unwrap();
        let src = t.path().join("src-helper");
        fs::write(&src, b"#!/bin/sh\n").unwrap();
        let l = Layout {
            helper: dir.join("wglane-helper"),
            sudoers: sudoers_d.join("wglane"),
            owner_uid: fs::metadata(t.path()).unwrap().uid(),
        };
        (t, l, src)
    }

    #[test]
    fn install_copies_validates_then_renames() {
        let (_t, l, src) = fixture();
        let r = FakeRunner::new();
        install_at(&r, &l, "kn", &src).unwrap();
        let calls = r.calls();
        let progs: Vec<_> = calls
            .iter()
            .map(|c| (c[0].as_str(), c[1].as_str()))
            .collect();
        assert_eq!(
            progs,
            [
                (CHOWN, "root:wheel"),
                (VISUDO, "-cf"),
                (CHOWN, "root:wheel")
            ]
        );
        assert_eq!(fs::read(&l.helper).unwrap(), b"#!/bin/sh\n");
        assert_eq!(fs::metadata(&l.helper).unwrap().mode() & 0o7777, 0o755);
        assert_eq!(fs::metadata(&l.sudoers).unwrap().mode() & 0o7777, 0o440);
        assert_eq!(
            fs::read_to_string(&l.sudoers).unwrap(),
            format!("kn ALL=(root) NOPASSWD: {}\n", l.helper.display())
        );
        // No temp leftovers.
        assert_eq!(
            fs::read_dir(l.sudoers.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn visudo_failure_installs_no_rule() {
        let (_t, l, src) = fixture();
        let r = FakeRunner::new();
        r.push_stdout("");
        r.push(CommandOutput {
            code: Some(1),
            stderr: "syntax error".into(),
            ..Default::default()
        });
        assert!(install_at(&r, &l, "kn", &src).is_err());
        assert!(!l.sudoers.exists());
        assert_eq!(
            fs::read_dir(l.sudoers.parent().unwrap()).unwrap().count(),
            0
        );
    }

    #[test]
    fn bad_user_touches_nothing() {
        let (_t, l, src) = fixture();
        let r = FakeRunner::new();
        assert!(install_at(&r, &l, "bad user", &src).is_err());
        assert!(r.calls().is_empty() && !l.helper.exists());
    }

    #[test]
    fn refuses_foreign_owned_or_writable_dir() {
        let (_t, mut l, src) = fixture();
        l.owner_uid += 1;
        assert!(install_at(&FakeRunner::new(), &l, "kn", &src).is_err());
        l.owner_uid -= 1;
        fs::set_permissions(
            l.helper.parent().unwrap(),
            fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        assert!(install_at(&FakeRunner::new(), &l, "kn", &src).is_err());
    }

    #[test]
    fn uninstall_removes_both_and_is_idempotent() {
        let (_t, l, src) = fixture();
        install_at(&FakeRunner::new(), &l, "kn", &src).unwrap();
        uninstall_at(&l).unwrap();
        uninstall_at(&l).unwrap();
        assert!(!l.helper.exists() && !l.sudoers.exists());
    }
}
