//! A failed doctor check is an expected result: table, exit 1, no error report.

use std::process::Command;

#[test]
fn failing_doctor_exits_1_without_an_error_report() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_wglane"))
        .args(["doctor", "--config-dir"])
        .arg(dir.path().join("missing"))
        .env_remove("NORDVPN_TOKEN")
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(stdout.contains("FAIL  config dir"), "{stdout}");
    assert!(
        !stderr.contains("Error:") && !stderr.contains("Location"),
        "{stderr}"
    );
}
