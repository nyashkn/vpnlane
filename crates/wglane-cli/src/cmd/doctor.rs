//! `wglane doctor`: check prerequisites and print the fix for each failure.

use std::fmt::Write as _;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use color_eyre::Result;
use serde_json::json;
use wglane_core::Error;
use wglane_core::ports::{TokenSource, UdpProbe};
use wglane_macos::{
    CommandRunner, HELPER_PATH, SUDOERS_PATH, SystemRunner, TOOL_DIR, UdpSend, check_tool_dir,
};

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Check prerequisites and print how to fix each problem",
    long_about = "\
Checks: wireguard-tools installed, tailscale installed, helper installed
root-owned with its sudoers rule, root-owned tool directory
(wg, wireguard-go), token present with mode 600, config
directory private to you, the leak check (warns if `exit_ip_check = false`),
and outbound UDP 51820 (NordLynx).

Every failed check prints the exact command to fix it. The UDP check sends one
handshake-shaped packet to a server from a generated config. Nord never
answers it, so it can only prove this machine lets the packet out; whether the
network delivers it is reported as unknown (`??`, not a failure).",
    after_help = concat!("\
Examples:
  ", brand_name!(), " doctor
  ", brand_name!(), " doctor --json")
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
    /// Could not be decided; says what is and is not known. Not a failure.
    Unknown {
        note: String,
    },
    /// Works, but not as safe as it could be. Not a failure.
    Warn {
        note: String,
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
        (
            "helper tools",
            check_tools(Path::new(TOOL_DIR), Path::new("/"), 0),
        ),
        ("sudoers rule", check_sudoers(Path::new(SUDOERS_PATH))),
        ("token", check_token(&ctx.token_source())),
        ("config dir", check_config_dir(&ctx.config_dir)),
        ("leak check", check_leak(ctx)),
        (
            "udp 51820",
            check_udp(&UdpSend, ctx.conf_store().any_endpoint()),
        ),
    ];
    println!("{}", render(&rows, ctx.json)?);
    let failed = rows
        .iter()
        .filter(|(_, c)| matches!(c, Check::Fail { .. }))
        .count();
    if failed > 0 {
        // A failed check is a result, not a crash: no error report.
        std::process::exit(1);
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
        _ => fail(concat!("sudo ", brand_name!(), " install-helper")),
    }
}

/// The helper's tool dir and each tool in it are present and root-owned, as
/// are all ancestors up to `top`; the same check the helper runs before exec.
fn check_tools(dir: &Path, top: &Path, owner_uid: u32) -> Check {
    match check_tool_dir(dir, top, owner_uid) {
        Ok(()) => Check::Pass,
        Err(_) => fail(concat!("sudo ", brand_name!(), " install-helper")),
    }
}

/// The sudoers file is root-only (0440), so only its existence is checked.
fn check_sudoers(path: &Path) -> Check {
    if path.exists() {
        Check::Pass
    } else {
        fail(concat!("sudo ", brand_name!(), " install-helper"))
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

const UDP_WHY: &str = concat!(
    "NordLynx only speaks UDP 51820, so `",
    brand_name!(),
    " up` will connect but never \
    handshake on a network that blocks it (hotspots, campus, hotel, some ISPs). Options: use \
    another network or a phone hotspot; ask the network owner to allow outbound UDP 51820; \
    check local firewall/pf rules; or use the Nord app's NordWhisper, which ",
    brand_name!(),
    " does not \
    support (see docs/notes/udp-blocked-fallback.md)"
);

/// A WireGuard server never answers an unauthenticated initiation, so the most
/// this can prove is that the local stack lets the datagram out.
fn check_udp<U: UdpProbe>(udp: &U, endpoint: Option<std::net::SocketAddr>) -> Check {
    let Some(endpoint) = endpoint else {
        return Check::Unknown {
            note: concat!(
                "no generated config to take an endpoint from; run `",
                brand_name!(),
                " gen <CC>` and retry"
            )
            .into(),
        };
    };
    match udp.send_probe(endpoint) {
        Ok(()) => Check::Unknown {
            note: concat!(
                "packet left this machine; delivery cannot be tested without a tunnel. \
                   If `",
                brand_name!(),
                " up` never handshakes, this network may block UDP 51820"
            )
            .into(),
        },
        Err(e) => fail(format!("{e}. {UDP_WHY}")),
    }
}

/// The UDP 51820 check for the TUI: `None` with no generated config to take
/// an endpoint from, `Some(true)` when the datagram could not leave.
#[cfg(feature = "tui")]
pub(crate) fn udp_blocked(ctx: &Context) -> Option<bool> {
    let endpoint = ctx.conf_store().any_endpoint()?;
    Some(matches!(
        check_udp(&UdpSend, Some(endpoint)),
        Check::Fail { .. }
    ))
}

/// Holds the token and generated configs, so group/world access is a leak.
/// The exit IP lookup (`exit_ip_check`) is on. Off is allowed, so only a warning.
fn check_leak(ctx: &Context) -> Check {
    match ctx.settings(wglane_core::Overrides::default()) {
        Ok(s) => leak_check(s.exit_ip_check.value),
        Err(e) => Check::Unknown {
            note: format!("settings unreadable: {e}"),
        },
    }
}

fn leak_check(enabled: bool) -> Check {
    if enabled {
        Check::Pass
    } else {
        Check::Warn {
            note: concat!("exit_ip_check = false: `status` cannot tell you if traffic leaks; remove the key or set ", brand_env!("EXIT_IP_CHECK"), "=true").to_owned(),
        }
    }
}

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
                Check::Unknown { note } => {
                    json!({ "check": name, "status": "unknown", "note": note })
                }
                Check::Warn { note } => json!({ "check": name, "status": "warn", "note": note }),
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
            Check::Unknown { note } => writeln!(out, "??    {name:<width$}  {note}"),
            Check::Warn { note } => writeln!(out, "WARN  {name:<width$}  {note}"),
            Check::Fail { fix } => writeln!(out, "FAIL  {name:<width$}  fix: {fix}"),
        }
        .expect("writing to a String cannot fail");
    }
    Ok(out.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use wglane_core::testing::{FakeToken, FakeUdp};

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
    fn tool_dir_missing_or_untrusted_fails_with_install_fix() {
        let d = tmp("tools");
        let top = fs::canonicalize(&d).unwrap();
        let uid = fs::metadata(&d).unwrap().uid();
        let fix = fail("sudo wglane install-helper");
        assert_eq!(check_tools(&d.join("bin"), &top, uid), fix, "missing");
        let bin = d.join("bin");
        fs::create_dir(&bin).unwrap();
        assert_eq!(check_tools(&bin, &top, uid), fix, "empty");
        for t in ["wg", "wireguard-go"] {
            fs::write(bin.join(t), t).unwrap();
        }
        assert_eq!(check_tools(&bin, &top, uid), Check::Pass);
        assert_eq!(check_tools(&bin, &top, uid + 1), fix, "wrong owner");
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(check_tools(&bin, &top, uid), fix, "writable");
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
    fn udp_unknown_without_endpoint_or_when_sent_and_fail_when_refused() {
        let ep = "203.0.113.9:51820".parse().unwrap();
        let ok = FakeUdp::default();
        assert!(matches!(check_udp(&ok, None), Check::Unknown { .. }));
        assert!(ok.sent.borrow().is_empty(), "no endpoint, nothing sent");
        assert!(matches!(check_udp(&ok, Some(ep)), Check::Unknown { .. }));
        assert_eq!(*ok.sent.borrow(), vec![ep]);
        let no = FakeUdp {
            refuse: Some(std::io::ErrorKind::NetworkUnreachable),
            ..Default::default()
        };
        let Check::Fail { fix } = check_udp(&no, Some(ep)) else {
            panic!("expected fail");
        };
        assert!(
            fix.contains("sending UDP to 203.0.113.9:51820") && fix.contains("another network")
        );
        insta::assert_snapshot!(render(&[("udp 51820", Check::Fail { fix })], false).unwrap());
    }

    #[test]
    fn a_disabled_leak_check_warns_but_does_not_fail() {
        assert_eq!(leak_check(true), Check::Pass);
        let off = leak_check(false);
        assert!(matches!(off, Check::Warn { .. }));
        let rows = [("leak check", off)];
        insta::assert_snapshot!("leak_warn_text", render(&rows, false).unwrap());
        assert!(
            render(&rows, true)
                .unwrap()
                .contains("\"status\": \"warn\"")
        );
    }

    #[test]
    fn output_is_stable() {
        let rows = [
            ("wireguard-tools", Check::Pass),
            ("helper", fail("sudo wglane install-helper")),
            ("config dir", fail("chmod 700 /home/u/.config/wglane")),
            (
                "udp 51820",
                check_udp(
                    &FakeUdp::default(),
                    Some("203.0.113.9:51820".parse().unwrap()),
                ),
            ),
        ];
        insta::assert_snapshot!(render(&rows, false).unwrap());
        insta::assert_snapshot!(render(&rows, true).unwrap());
    }
}
