//! `wglane doctor`: check prerequisites and print the fix for each failure.

use std::fmt::Write as _;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use serde_json::json;
use wglane_core::Error;
use wglane_core::ports::TokenSource;
use wglane_macos::{CommandRunner, HELPER_PATH, SUDOERS_PATH, SystemRunner};

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Check prerequisites and print how to fix each problem",
    long_about = "\
Checks: wireguard-tools installed, tailscale installed, helper installed
root-owned with its sudoers rule, token present with mode 600, config
directory private to you.

Every failed check prints the exact command to fix it.",
    after_help = "\
Examples:
  wglane doctor
  wglane doctor --json"
)]
pub(crate) struct Args {}

/// Outcome of one check.
#[derive(Debug, PartialEq, Eq)]
enum Check {
    Pass,
    /// Failed; `fix` is the exact command (or instruction) that repairs it.
    Fail {
        fix: String,
    },
}

fn fail(fix: impl Into<String>) -> Check {
    Check::Fail { fix: fix.into() }
}

pub(crate) fn run(ctx: &Context, _args: &Args) -> Result<()> {
    let rows = [
        (
            "wireguard-tools",
            check_program(&SystemRunner, "wg", "brew install wireguard-tools"),
        ),
        (
            "tailscale",
            check_program(&SystemRunner, "tailscale", "brew install tailscale"),
        ),
        ("helper", check_helper(Path::new(HELPER_PATH), 0)),
        ("sudoers rule", check_sudoers(Path::new(SUDOERS_PATH))),
        ("token", check_token(&ctx.token_source())),
        ("config dir", check_config_dir(&ctx.config_dir)),
    ];
    println!("{}", render(&rows, ctx.json)?);
    let failed = rows.iter().filter(|(_, c)| *c != Check::Pass).count();
    if failed > 0 {
        return Err(eyre!("{failed} check(s) failed"));
    }
    Ok(())
}

/// `program --version` starts, whatever its exit status.
fn check_program<R: CommandRunner>(runner: &R, program: &str, fix: &str) -> Check {
    match runner.run(program, &["--version"]) {
        Ok(_) => Check::Pass,
        Err(_) => fail(fix),
    }
}

/// The helper exists, is owned by `owner_uid` and is not group/world-writable
/// (a sudoers rule on a user-writable binary is a root exploit).
fn check_helper(path: &Path, owner_uid: u32) -> Check {
    match std::fs::metadata(path) {
        Ok(m) if m.uid() == owner_uid && m.permissions().mode() & 0o022 == 0 => Check::Pass,
        _ => fail("sudo wglane install-helper"),
    }
}

/// The sudoers file is root-only (0440), so only its existence is checked.
fn check_sudoers(path: &Path) -> Check {
    if path.exists() {
        Check::Pass
    } else {
        fail("sudo wglane install-helper")
    }
}

fn check_token<T: TokenSource>(source: &T) -> Check {
    match source.token() {
        Ok(Some(_)) => Check::Pass,
        Ok(None) => fail(source.describe()),
        Err(Error::TokenInsecure { path, .. }) => fail(format!("chmod 600 {}", path.display())),
        Err(e) => fail(e.to_string()),
    }
}

/// Holds the token and generated configs, so group/world access is a leak.
fn check_config_dir(dir: &Path) -> Check {
    let d = dir.display();
    match std::fs::metadata(dir) {
        Ok(m) if m.permissions().mode() & 0o077 != 0 => fail(format!("chmod 700 {d}")),
        Ok(_) => Check::Pass,
        Err(_) => fail(format!("mkdir -p -m 700 {d}")),
    }
}

fn render(rows: &[(&str, Check)], json: bool) -> Result<String> {
    if json {
        let v: Vec<_> = rows
            .iter()
            .map(|(name, c)| match c {
                Check::Pass => json!({ "check": name, "status": "pass" }),
                Check::Fail { fix } => json!({ "check": name, "status": "fail", "fix": fix }),
            })
            .collect();
        return Ok(serde_json::to_string_pretty(&v)?);
    }
    let width = rows.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (name, c) in rows {
        match c {
            Check::Pass => writeln!(out, "ok    {name}"),
            Check::Fail { fix } => writeln!(out, "FAIL  {name:<width$}  fix: {fix}"),
        }
        .expect("writing to a String cannot fail");
    }
    Ok(out.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use wglane_core::testing::FakeToken;

    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wglane-doctor-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn program_missing_fails_with_fix() {
        struct Missing;
        impl CommandRunner for Missing {
            fn run(&self, p: &str, _: &[&str]) -> wglane_core::Result<wglane_macos::CommandOutput> {
                Err(Error::io(p, std::io::ErrorKind::NotFound.into()))
            }
        }
        let runner = wglane_macos::testing::FakeRunner::new();
        assert_eq!(check_program(&runner, "wg", "x"), Check::Pass);
        assert_eq!(runner.calls(), vec![vec!["wg", "--version"]]);
        assert_eq!(
            check_program(&Missing, "wg", "brew install y"),
            fail("brew install y")
        );
    }

    #[test]
    fn helper_must_exist_be_owned_and_not_writable() {
        let d = tmp("helper");
        let f = d.join("helper");
        assert_eq!(check_helper(&f, 0), fail("sudo wglane install-helper"));
        fs::write(&f, b"x").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
        let uid = fs::metadata(&f).unwrap().uid();
        assert_eq!(check_helper(&f, uid), Check::Pass);
        assert_ne!(check_helper(&f, uid + 1), Check::Pass);
        fs::set_permissions(&f, fs::Permissions::from_mode(0o775)).unwrap();
        assert_ne!(check_helper(&f, uid), Check::Pass);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn sudoers_and_token_and_config_dir() {
        let d = tmp("misc");
        assert_ne!(check_sudoers(&d.join("nope")), Check::Pass);
        assert_eq!(check_sudoers(&d), Check::Pass);

        assert_eq!(check_token(&FakeToken(Some("a".repeat(64)))), Check::Pass);
        assert!(matches!(check_token(&FakeToken(None)), Check::Fail { .. }));

        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            check_config_dir(&d),
            fail(format!("chmod 700 {}", d.display()))
        );
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(check_config_dir(&d), Check::Pass);
        let gone = d.join("gone");
        assert_eq!(
            check_config_dir(&gone),
            fail(format!("mkdir -p -m 700 {}", gone.display()))
        );
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn output_is_stable() {
        let rows = [
            ("wireguard-tools", Check::Pass),
            ("helper", fail("sudo wglane install-helper")),
            ("config dir", fail("chmod 700 /home/u/.config/wglane")),
        ];
        insta::assert_snapshot!(render(&rows, false).unwrap());
        insta::assert_snapshot!(render(&rows, true).unwrap());
    }
}
