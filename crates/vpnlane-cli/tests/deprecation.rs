//! Old spellings still work, and say so exactly once per run on stderr.

use std::process::Command;

fn vpnlane(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vpnlane"));
    cmd.args(args)
        .arg("--config-dir")
        .arg(dir)
        .env_remove("VPNLANE_COUNTRY")
        .env_remove("VPNLANE_LOCATION")
        .env_remove("VPNLANE_PRESET")
        .env_remove("VPNLANE_PICK");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run vpnlane");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn old_keys_and_env_map_to_location_with_one_stderr_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("config.toml"),
        "default_country = \"US\"\ndefault_city = \"Chicago\"\n[presets.work]\ncountry = \"DE\"\n",
    )
    .expect("write config");
    let (out, err) = vpnlane(
        dir.path(),
        &["config", "show"],
        &[("VPNLANE_COUNTRY", "fr")],
    );
    let lines: Vec<_> = err.lines().filter(|l| l.contains("deprecated")).collect();
    assert_eq!(lines.len(), 1, "{err}");
    for named in [
        "default_country, default_city -> location",
        "VPNLANE_COUNTRY -> VPNLANE_LOCATION",
    ] {
        assert!(lines[0].contains(named), "{}", lines[0]);
    }
    assert!(
        out.contains("location               FR"),
        "env alias wins: {out}"
    );
    assert!(
        out.contains("presets.work.country -> presets.work.location"),
        "{out}"
    );
}

#[test]
fn new_spellings_print_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("config.toml"),
        "location = \"US/Chicago\"\n",
    )
    .expect("write config");
    let (out, err) = vpnlane(
        dir.path(),
        &["config", "show"],
        &[("VPNLANE_LOCATION", "DE")],
    );
    assert!(!err.contains("deprecated"), "{err}");
    assert!(!out.contains("deprecated"), "{out}");
    assert!(out.contains("location               DE"), "{out}");
}
