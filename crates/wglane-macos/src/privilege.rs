use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use wglane_core::{Error, Result};

use crate::helper::{TOOL_DIR, TOOLS};
use crate::{CommandRunner, HELPER_PATH};

/// sudoers drop-in written by `install-helper`.
pub const SUDOERS_PATH: &str = "/etc/sudoers.d/wglane";

const CHOWN: &str = "/usr/sbin/chown";
const VISUDO: &str = "/usr/sbin/visudo";
const OTOOL: &str = "/usr/bin/otool";
const LS: &str = "/bin/ls";
/// The only places a root-run binary may load libraries from (SIP-protected).
const SYSTEM_LIBS: [&str; 2] = ["/usr/lib/", "/System/Library/"];

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

/// `(uid, st_mode)` of `path` without following a final symlink.
fn lstat(path: &Path) -> io::Result<(u32, u32)> {
    let m = fs::symlink_metadata(path)?;
    Ok((m.uid(), m.mode()))
}

/// Fails unless `path` and every ancestor up to and including `top` has uid
/// `owner` and is not group/world-writable. A symlink fails (its mode is 0777),
/// so pass a canonical path. `stat` is the seam: `(uid, st_mode)` per path.
fn require_trusted_chain(
    path: &Path,
    top: &Path,
    owner: u32,
    stat: impl Fn(&Path) -> io::Result<(u32, u32)>,
) -> Result<()> {
    for p in path.ancestors() {
        let (uid, mode) = stat(p).map_err(|e| Error::io(format!("stat {}", p.display()), e))?;
        if uid != owner || mode & 0o022 != 0 {
            return Err(invalid(format!(
                "{} must be owned by root and not group/world-writable (uid {uid}, mode {:o}); run `sudo chown root:wheel {p} && sudo chmod go-w {p}`",
                p.display(),
                mode & 0o7777,
                p = p.display()
            )));
        }
        if p == top {
            break;
        }
    }
    Ok(())
}

/// `ls -le` shows an ACL entry as an indented `N: user:... allow ...` line.
fn listing_has_acl(ls: &str) -> bool {
    ls.lines().any(|l| {
        l.trim_start()
            .split_once(": ")
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Fails if `path` or any ancestor up to `top` has an extended ACL: an allow
/// ACE can grant write access that the mode bits do not show. Fails closed if
/// `ls` cannot be run.
fn require_no_acl(path: &Path, top: &Path) -> Result<()> {
    let mut chain = Vec::new();
    for p in path.ancestors() {
        chain.push(p);
        if p == top {
            break;
        }
    }
    let out = Command::new(LS)
        .arg("-lde")
        .args(&chain)
        .env_clear()
        .output()
        .map_err(|e| Error::io(format!("running {LS}"), e))?;
    if !out.status.success() {
        return Err(invalid(format!(
            "{LS} failed checking ACLs on {}",
            path.display()
        )));
    }
    if listing_has_acl(&String::from_utf8_lossy(&out.stdout)) {
        return Err(invalid(format!(
            "{} or a parent directory has an ACL; run `sudo chmod -N` on it",
            path.display()
        )));
    }
    Ok(())
}

/// Run-time check before root executes `path`: it must resolve (symlinks
/// followed) to a regular file inside `tool_dir`, and the file and all its
/// ancestors up to `top` must be owned by `owner`, not g/o-writable and free
/// of ACLs. Returns the canonical path.
pub(crate) fn trusted_tool(
    path: &Path,
    tool_dir: &Path,
    top: &Path,
    owner: u32,
) -> Result<PathBuf> {
    let canon = trusted_tool_with(path, tool_dir, top, owner, lstat)?;
    require_no_acl(&canon, top)?;
    Ok(canon)
}

/// [`trusted_tool`] without the ACL check, with `stat` as the seam.
fn trusted_tool_with(
    path: &Path,
    tool_dir: &Path,
    top: &Path,
    owner: u32,
    stat: impl Fn(&Path) -> io::Result<(u32, u32)>,
) -> Result<PathBuf> {
    let canon = fs::canonicalize(path).map_err(|e| {
        Error::io(
            format!(
                "finding {} (run `sudo wglane install-helper`)",
                path.display()
            ),
            e,
        )
    })?;
    if !canon.starts_with(tool_dir) {
        return Err(invalid(format!(
            "{} resolves to {}, outside {}",
            path.display(),
            canon.display(),
            tool_dir.display()
        )));
    }
    require_trusted_chain(&canon, top, owner, &stat)?;
    // S_IFMT == S_IFREG
    if stat(&canon)
        .map_err(|e| Error::io(format!("stat {}", canon.display()), e))?
        .1
        & 0o170_000
        != 0o100_000
    {
        return Err(invalid(format!(
            "{} is not a regular file",
            canon.display()
        )));
    }
    Ok(canon)
}

/// Fails unless each helper tool (wg, wireguard-go) in `dir` is
/// a root-owned regular file under a trusted chain: the helper's run-time check.
pub fn check_tool_dir(dir: &Path, top: &Path, owner: u32) -> Result<()> {
    let dir = fs::canonicalize(dir).map_err(|e| {
        Error::io(
            format!(
                "finding {} (run `sudo wglane install-helper`)",
                dir.display()
            ),
            e,
        )
    })?;
    TOOLS
        .iter()
        .try_for_each(|t| trusted_tool(&dir.join(t), &dir, top, owner).map(drop))
}

/// Fails unless `otool -L` output (`otool_l`) for `name` lists only libraries
/// under [`SYSTEM_LIBS`]. Anything else (`@rpath`, Homebrew, `..` tricks) is a
/// library a non-root user could replace, and root would load it.
fn check_dependencies(name: &str, otool_l: &str) -> Result<()> {
    let mut headers = 0;
    for line in otool_l.lines() {
        if !line.starts_with(char::is_whitespace) {
            // "<file>:" (one per architecture of a fat binary)
            headers += usize::from(line.ends_with(':'));
            continue;
        }
        let dep = line.trim().split(" (").next().unwrap_or_default();
        let clean = Path::new(dep)
            .components()
            .all(|c| !matches!(c, Component::ParentDir));
        if !clean || !SYSTEM_LIBS.iter().any(|p| dep.starts_with(p)) {
            return Err(invalid(format!(
                "{name} loads {dep}, which is outside /usr/lib and /System/Library; refusing to run it as root"
            )));
        }
    }
    if headers == 0 {
        return Err(invalid(format!("unexpected `otool -L` output for {name}")));
    }
    Ok(())
}

/// Runs `otool -L` on `path` and applies [`check_dependencies`].
fn verify_dependencies<R: CommandRunner>(runner: &R, path: &Path) -> Result<()> {
    let out = runner.run_ok(OTOOL, &["-L", &path.to_string_lossy()])?;
    check_dependencies(&path.display().to_string(), &out.stdout)
}

/// Where [`install_helper`] writes. Production values come from [`Layout::system`];
/// tests point it into a temp dir.
struct Layout {
    helper: PathBuf,
    sudoers: PathBuf,
    /// Root-owned copies of [`TOOLS`] live here.
    tools: PathBuf,
    /// Where the tools are copied from, in order (Homebrew prefixes).
    tool_sources: Vec<PathBuf>,
    /// Uid that must own the helper's and tools' directories and all their ancestors (0 in production).
    owner_uid: u32,
    /// Last ancestor checked ("/" in production).
    top: PathBuf,
}

impl Layout {
    fn system() -> Self {
        Self {
            helper: HELPER_PATH.into(),
            sudoers: SUDOERS_PATH.into(),
            tools: TOOL_DIR.into(),
            tool_sources: vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()],
            owner_uid: 0,
            top: "/".into(),
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
/// [`crate::HELPER_PATH`] root:wheel 0755, copy `wg` and `wireguard-go` from
/// Homebrew into root:wheel [`TOOL_DIR`], and write [`SUDOERS_PATH`] 0440 for
/// `user` after `visudo -cf` validates it.
///
/// Every copied binary must pass `otool -L` loading only system libraries
/// (checked on the copy, before it is renamed into place). Refuses unless the
/// effective uid is 0. Safe to re-run.
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
    match fs::remove_dir_all(&l.tools) {
        Err(e) if e.kind() != ErrorKind::NotFound => {
            return Err(Error::io(format!("removing {}", l.tools.display()), e));
        }
        _ => {}
    }
    if let Some(parent) = l.tools.parent() {
        let _ = fs::remove_dir(parent); // only if now empty
    }
    Ok(())
}

fn install_at<R: CommandRunner>(runner: &R, l: &Layout, user: &str, source: &Path) -> Result<()> {
    // Render (and so validate) before touching the system.
    let rule = render_sudoers_rule(user, &l.helper)?;
    // Find every tool before touching the system.
    let sources = TOOLS
        .iter()
        .map(|t| {
            l.tool_sources
                .iter()
                .map(|d| d.join(t))
                .find(|p| p.is_file())
                .ok_or_else(|| {
                    invalid(format!(
                        "{t} not found in {:?}: brew install wireguard-tools wireguard-go",
                        l.tool_sources
                    ))
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let dir = l
        .helper
        .parent()
        .ok_or_else(|| invalid("helper path has no parent"))?;
    for d in [dir, &l.tools] {
        fs::create_dir_all(d).map_err(|e| Error::io(format!("creating {}", d.display()), e))?;
        // Canonical, so no symlink hides in the chain; then every ancestor is checked.
        let canon =
            fs::canonicalize(d).map_err(|e| Error::io(format!("resolving {}", d.display()), e))?;
        require_trusted_chain(&canon, &l.top, l.owner_uid, lstat)?;
        require_no_acl(&canon, &l.top)?;
    }

    // Tools first, so a half-finished install never leaves a sudoers rule without them.
    for (name, src) in TOOLS.iter().zip(&sources) {
        let dst = l.tools.join(name);
        let tmp = sibling(&dst, ".new");
        let mut src =
            File::open(src).map_err(|e| Error::io(format!("opening {}", src.display()), e))?;
        write_new(&tmp, 0o755, |f| io::copy(&mut src, f).map(drop))?;
        verify_dependencies(runner, &tmp).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })?;
        finish(runner, &tmp, &dst)?;
    }

    // Helper binary: temp file created O_EXCL (never follows a planted symlink), then rename.
    let tmp = sibling(&l.helper, ".new");
    let mut src =
        File::open(source).map_err(|e| Error::io(format!("opening {}", source.display()), e))?;
    write_new(&tmp, 0o755, |dst| io::copy(&mut src, dst).map(drop))?;
    verify_dependencies(runner, &tmp).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
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

    const GOOD_DEPS: &str = "/x/tool:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)\n\t/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0)\n";
    const HOMEBREW_DEPS: &str = "/x/tool:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n\t/opt/homebrew/opt/gettext/lib/libintl.8.dylib (compatibility version 12.0.0)\n";

    /// [`FakeRunner`] whose `otool -L` prints `otool`.
    struct Fake {
        inner: FakeRunner,
        otool: &'static str,
    }

    impl Fake {
        fn new() -> Self {
            Self::printing(GOOD_DEPS)
        }

        fn printing(otool: &'static str) -> Self {
            Self {
                inner: FakeRunner::new(),
                otool,
            }
        }
    }

    impl CommandRunner for Fake {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
            let mut out = self.inner.run(program, args)?;
            if program == OTOOL && out.success() {
                out.stdout = self.otool.into();
            }
            Ok(out)
        }
    }

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
        let brew = t.path().join("brew");
        fs::create_dir(&brew).unwrap();
        for tool in TOOLS {
            fs::write(brew.join(tool), tool).unwrap();
        }
        let l = Layout {
            helper: dir.join("wglane-helper"),
            sudoers: sudoers_d.join("wglane"),
            tools: dir.join("wglane/bin"),
            tool_sources: vec![brew],
            owner_uid: fs::metadata(t.path()).unwrap().uid(),
            top: fs::canonicalize(t.path()).unwrap(),
        };
        (t, l, src)
    }

    #[test]
    fn install_copies_validates_then_renames() {
        let (_t, l, src) = fixture();
        let r = Fake::new();
        install_at(&r, &l, "kn", &src).unwrap();
        let calls = r.inner.calls();
        let progs: Vec<_> = calls
            .iter()
            .map(|c| (c[0].as_str(), c[1].as_str()))
            .collect();
        assert_eq!(
            progs,
            [
                // Each binary is checked as copied, then handed to root, in this order.
                (OTOOL, "-L"),
                (CHOWN, "root:wheel"),
                (OTOOL, "-L"),
                (CHOWN, "root:wheel"),
                (OTOOL, "-L"),
                (CHOWN, "root:wheel"),
                (VISUDO, "-cf"),
                (CHOWN, "root:wheel")
            ]
        );
        for tool in TOOLS {
            let p = l.tools.join(tool);
            assert_eq!(fs::read(&p).unwrap(), tool.as_bytes());
            assert_eq!(fs::metadata(&p).unwrap().mode() & 0o7777, 0o755);
        }
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
        let r = Fake::new();
        for _ in 0..6 {
            r.inner.push_stdout("");
        }
        r.inner.push(CommandOutput {
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
        let r = Fake::new();
        assert!(install_at(&r, &l, "bad user", &src).is_err());
        assert!(r.inner.calls().is_empty() && !l.helper.exists());
    }

    #[test]
    fn refuses_foreign_owned_or_writable_dir() {
        let (_t, mut l, src) = fixture();
        l.owner_uid += 1;
        assert!(install_at(&Fake::new(), &l, "kn", &src).is_err());
        l.owner_uid -= 1;
        fs::set_permissions(
            l.helper.parent().unwrap(),
            fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        assert!(install_at(&Fake::new(), &l, "kn", &src).is_err());
    }

    #[test]
    fn missing_tool_touches_nothing() {
        let (_t, l, src) = fixture();
        fs::remove_file(l.tool_sources[0].join("wireguard-go")).unwrap();
        let r = Fake::new();
        let err = install_at(&r, &l, "kn", &src).unwrap_err().to_string();
        assert!(err.contains("wireguard-go not found"), "{err}");
        assert!(r.inner.calls().is_empty() && !l.helper.exists() && !l.tools.exists());
    }

    #[test]
    fn dependency_check_table() {
        let ok = |t: &str| check_dependencies("t", t).is_ok();
        assert!(ok(GOOD_DEPS));
        // Fat binary: one header per architecture.
        assert!(ok(
            "/x (architecture x86_64):\n\t/usr/lib/a.dylib (c)\n/x (architecture arm64):\n\t/usr/lib/a.dylib (c)\n"
        ));
        assert!(ok("/x:\n"), "statically linked");
        for bad in [
            HOMEBREW_DEPS,
            "/x:\n\t@rpath/libfoo.dylib (c)\n",
            "/x:\n\t@loader_path/libfoo.dylib (c)\n",
            "/x:\n\t@executable_path/../lib/libfoo.dylib (c)\n",
            "/x:\n\t/usr/local/lib/libfoo.dylib (c)\n",
            "/x:\n\t/usr/lib/../../opt/homebrew/lib/libfoo.dylib (c)\n",
            "/x:\n\t/usr/libexec/evil.dylib (c)\n",
            "/x:\n\t/System/Libraryx/evil.dylib (c)\n",
            "/x:\n\t/usr/lib/libSystem.B.dylib (c)\n\t/tmp/evil.dylib (c)\n",
            "",
            "not otool output",
        ] {
            assert!(!ok(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn dependency_outside_system_libs_installs_nothing() {
        // The finding: a Homebrew dylib (a user-writable path) reachable from a root-run binary.
        let (_t, l, src) = fixture();
        let r = Fake::printing(HOMEBREW_DEPS);
        let err = install_at(&r, &l, "kn", &src).unwrap_err().to_string();
        assert!(err.contains("/opt/homebrew/opt/gettext"), "{err}");
        assert!(!l.sudoers.exists() && !l.helper.exists());
        assert!(
            fs::read_dir(&l.tools).unwrap().next().is_none(),
            "no copy left behind"
        );
        assert_eq!(r.inner.calls().len(), 1, "stops at the first bad binary");
    }

    #[test]
    fn failing_otool_on_the_helper_binary_writes_no_sudoers() {
        let (_t, l, src) = fixture();
        let r = Fake::new();
        // Both tools pass (otool, chown each); the helper's otool (5th call) fails.
        for _ in 0..4 {
            r.inner.push_stdout("");
        }
        r.inner.push(CommandOutput {
            code: Some(1),
            stderr: "boom".into(),
            ..Default::default()
        });
        assert!(install_at(&r, &l, "kn", &src).is_err());
        assert!(!l.sudoers.exists() && !l.helper.exists());
    }

    #[test]
    fn check_tool_dir_needs_every_tool_trusted() {
        let (t, l, src) = fixture();
        let top = fs::canonicalize(t.path()).unwrap();
        let uid = l.owner_uid;
        assert!(check_tool_dir(&l.tools, &top, uid).is_err(), "missing dir");
        install_at(&Fake::new(), &l, "kn", &src).unwrap();
        check_tool_dir(&l.tools, &top, uid).unwrap();
        assert!(
            check_tool_dir(&l.tools, &top, uid + 1).is_err(),
            "wrong owner"
        );
        fs::remove_file(l.tools.join("wg")).unwrap();
        assert!(
            check_tool_dir(&l.tools, &top, uid).is_err(),
            "one tool missing"
        );
    }

    #[test]
    fn refuses_untrusted_ancestor_of_the_helper_dir() {
        // `top` is the tempdir; its child `mid` is group-writable above libexec.
        let (t, mut l, src) = fixture();
        let mid = t.path().join("mid");
        fs::create_dir(&mid).unwrap();
        fs::set_permissions(&mid, fs::Permissions::from_mode(0o775)).unwrap();
        l.helper = mid.join("libexec/wglane-helper");
        l.tools = mid.join("libexec/wglane/bin");
        let err = install_at(&Fake::new(), &l, "kn", &src).unwrap_err();
        assert!(err.to_string().contains("mid"), "{err}");
    }

    /// Table over the ancestor walk with a fake `stat`: (path, uid, mode).
    #[test]
    fn chain_check_table() {
        const OK: u32 = 0o040_755;
        let chain = |entries: &'static [(&'static str, u32, u32)]| {
            move |p: &Path| {
                entries
                    .iter()
                    .find(|(q, ..)| Path::new(q) == p)
                    .map(|&(_, uid, mode)| (uid, mode))
                    .ok_or_else(|| io::Error::from(ErrorKind::NotFound))
            }
        };
        let f = Path::new("/usr/local/libexec/wglane/bin/wg-quick");
        let top = Path::new("/");
        let good: &[(&str, u32, u32)] = &[
            ("/", 0, OK),
            ("/usr", 0, OK),
            ("/usr/local", 0, OK),
            ("/usr/local/libexec", 0, OK),
            ("/usr/local/libexec/wglane", 0, OK),
            ("/usr/local/libexec/wglane/bin", 0, OK),
            ("/usr/local/libexec/wglane/bin/wg-quick", 0, 0o100_755),
        ];
        assert!(require_trusted_chain(f, top, 0, chain(good.to_vec().leak())).is_ok());
        // Each variation breaks exactly one entry.
        for (i, (path, _, _)) in good.iter().enumerate() {
            for (what, uid, mode) in [
                ("non-root owner", 501, good[i].2),
                ("group-writable", 0, good[i].2 | 0o020),
                ("other-writable", 0, good[i].2 | 0o002),
                ("symlink", 0, 0o120_777),
            ] {
                let mut t = good.to_vec();
                t[i] = (path, uid, mode);
                let r = require_trusted_chain(f, top, 0, chain(t.leak()));
                assert!(r.is_err(), "{what} at {path} accepted");
            }
        }
        // Unstattable ancestor fails closed.
        assert!(require_trusted_chain(f, top, 0, chain(&good[3..])).is_err());
    }

    #[test]
    fn trusted_tool_refuses_symlink_out_and_writable_dir() {
        let t = tempfile::tempdir().unwrap();
        let top = fs::canonicalize(t.path()).unwrap();
        let uid = fs::metadata(&top).unwrap().uid();
        let tools = top.join("bin");
        fs::create_dir(&tools).unwrap();
        let outside = top.join("elsewhere");
        fs::write(&outside, "x").unwrap();
        fs::write(tools.join("ok"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, tools.join("link")).unwrap();
        assert_eq!(
            trusted_tool(&tools.join("ok"), &tools, &top, uid).unwrap(),
            tools.join("ok")
        );
        let err = trusted_tool(&tools.join("link"), &tools, &top, uid).unwrap_err();
        assert!(err.to_string().contains("outside"), "{err}");
        assert!(trusted_tool(&tools.join("missing"), &tools, &top, uid).is_err());
        // Wrong owner (as if a user owned it) and a group-writable file.
        assert!(trusted_tool(&tools.join("ok"), &tools, &top, uid + 1).is_err());
        fs::set_permissions(tools.join("ok"), fs::Permissions::from_mode(0o775)).unwrap();
        assert!(trusted_tool(&tools.join("ok"), &tools, &top, uid).is_err());
    }

    #[test]
    fn trusted_tool_refuses_a_file_owned_by_another_uid_when_its_dirs_are_fine() {
        let t = tempfile::tempdir().unwrap();
        let top = fs::canonicalize(t.path()).unwrap();
        let uid = fs::metadata(&top).unwrap().uid();
        let tools = top.join("bin");
        fs::create_dir(&tools).unwrap();
        let tool = tools.join("wg");
        fs::write(&tool, "x").unwrap();
        // Everything is `uid`'s except the file itself.
        let stat = |p: &Path| lstat(p).map(|(u, m)| (if p == tool { uid + 1 } else { u }, m));
        let err = trusted_tool_with(&tool, &tools, &top, uid, stat).unwrap_err();
        assert!(err.to_string().contains("wg"), "{err}");
        assert!(trusted_tool_with(&tool, &tools, &top, uid, lstat).is_ok());
    }

    #[test]
    fn trusted_tool_refuses_non_regular_files() {
        let t = tempfile::tempdir().unwrap();
        let top = fs::canonicalize(t.path()).unwrap();
        let uid = fs::metadata(&top).unwrap().uid();
        let tools = top.join("bin");
        fs::create_dir_all(tools.join("sub")).unwrap();
        assert!(trusted_tool(&tools.join("sub"), &tools, &top, uid).is_err());
    }

    #[test]
    fn acl_listing_parser() {
        let plain = "-rwxr-xr-x  1 root  wheel  100 Oct 30 12:00 /a/wg\n";
        let acl =
            "-rwxr-xr-x+ 1 root  wheel  100 Oct 30 12:00 /a/wg\n 0: user:evil allow write,delete\n";
        assert!(!listing_has_acl(plain));
        assert!(listing_has_acl(acl));
        assert!(!listing_has_acl(
            "drwxr-xr-x  2 u g 64 Oct 30 12:00 /a/b: 1: x\n"
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn trusted_tool_refuses_an_extended_acl_on_the_file_or_a_parent() {
        let t = tempfile::tempdir().unwrap();
        let top = fs::canonicalize(t.path()).unwrap();
        let uid = fs::metadata(&top).unwrap().uid();
        let tools = top.join("bin");
        fs::create_dir(&tools).unwrap();
        let tool = tools.join("wg");
        fs::write(&tool, "x").unwrap();
        assert!(trusted_tool(&tool, &tools, &top, uid).is_ok());
        // An ACE granting write on the directory: the mode bits still read 0755.
        let ace = "everyone allow add_file,delete_child";
        let chmod = |path: &Path| {
            Command::new("/bin/chmod")
                .args(["+a", ace])
                .arg(path)
                .status()
                .unwrap()
                .success()
        };
        assert!(chmod(&tools));
        let err = trusted_tool(&tool, &tools, &top, uid).unwrap_err();
        assert!(err.to_string().contains("ACL"), "{err}");
        Command::new("/bin/chmod")
            .arg("-N")
            .arg(&tools)
            .status()
            .unwrap();
        assert!(chmod(&tool));
        assert!(trusted_tool(&tool, &tools, &top, uid).is_err());
    }

    #[test]
    fn uninstall_removes_both_and_is_idempotent() {
        let (_t, l, src) = fixture();
        install_at(&Fake::new(), &l, "kn", &src).unwrap();
        uninstall_at(&l).unwrap();
        uninstall_at(&l).unwrap();
        assert!(!l.helper.exists() && !l.sudoers.exists() && !l.tools.exists());
    }
}
