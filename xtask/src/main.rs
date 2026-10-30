//! `cargo xtask <task>`: repo automation in Rust instead of shell scripts.

#[macro_use]
extern crate vpnlane_core;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use color_eyre::Result;
use color_eyre::eyre::{WrapErr, bail};

#[derive(Parser)]
#[command(about = concat!(brand_name!(), " development tasks"))]
struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Write bash/zsh/fish completions to target/assets/completions/.
    Completions,
    /// Write man pages (one per subcommand) to target/assets/man/.
    Man,
    /// Build release binaries (vpnlane, vpnlane-helper) and print their paths.
    Release,
    /// Fail if the release-please manifest, workspace version and Cargo.lock entries disagree.
    CheckVersions,
    /// Needs network: fail (exit 2) if Nord lists countries missing from the TUI continent table.
    CheckContinents,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let out = workspace_root().join("target").join("assets");
    match Cli::parse().task {
        Task::Completions => completions(&out.join("completions")),
        Task::Man => man(&out.join("man")),
        Task::Release => release(&workspace_root()),
        Task::CheckVersions => check_versions_files(&workspace_root()),
        Task::CheckContinents => check_continents(&workspace_root()),
    }
}

const CONTINENT_FILE: &str = "crates/vpnlane-cli/src/tui/continent.rs";

fn check_continents(root: &Path) -> Result<()> {
    use vpnlane_core::ports::ServerCatalog;
    let src = fs::read_to_string(root.join(CONTINENT_FILE)).wrap_err(CONTINENT_FILE)?;
    let table = table_codes(&src);
    let live = vpnlane_nord::NordApi::new().countries()?;
    let missing: Vec<&str> = live
        .iter()
        .map(|c| c.code.as_str())
        .filter(|c| !table.iter().any(|t| t == c))
        .collect();
    if missing.is_empty() {
        println!(
            "continent table ok: {} live countries, none missing",
            live.len()
        );
        return Ok(());
    }
    println!(
        "continent table drift: {} missing: {}",
        missing.len(),
        missing.join(" ")
    );
    std::process::exit(2) // 2 = drift, so CI can tell it from a network failure (1)
}

/// The quoted ISO codes in the `match` of `continent()`.
// shortcut: text scan of the match arms, not a parser; fine while the table stays one match.
fn table_codes(src: &str) -> Vec<String> {
    let body = src
        .split("match code.as_str() {")
        .nth(1)
        .unwrap_or_default();
    let body = body.split("_ => Other").next().unwrap_or_default();
    body.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn check_versions_files(root: &Path) -> Result<()> {
    let read = |name: &str| {
        fs::read_to_string(root.join(name)).wrap_err_with(|| format!("reading {name}"))
    };
    let problems = check_versions(
        &read(".release-please-manifest.json")?,
        &read("Cargo.toml")?,
        &read("Cargo.lock")?,
    );
    if problems.is_empty() {
        return Ok(());
    }
    bail!("version files disagree:\n  {}", problems.join("\n  "))
}

/// Every problem found; empty means manifest, `[workspace.package]` and lock entries agree.
fn check_versions(manifest: &str, cargo_toml: &str, lock: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let want = serde_json::from_str::<serde_json::Value>(manifest)
        .ok()
        .and_then(|v| v["."].as_str().map(str::to_owned));
    let Some(want) = want else {
        return vec![r#"manifest has no "." version"#.into()];
    };
    // shortcut: line-based scan, not a TOML parser; fine for this repo's flat layout.
    let ws = cargo_toml
        .split("[workspace.package]")
        .nth(1)
        .and_then(|rest| {
            rest.lines()
                .skip(1)
                .take_while(|l| !l.starts_with('['))
                .find_map(|l| quoted("version", l))
        });
    if ws.as_deref() != Some(&want) {
        problems.push(format!(
            "Cargo.toml workspace version {ws:?} != manifest {want}"
        ));
    }
    let mut name = None;
    let mut seen = 0;
    for line in lock.lines() {
        if let Some(n) = quoted("name", line) {
            name = Some(n);
        } else if let (Some(v), Some(n)) = (quoted("version", line), name.take())
            && (n.starts_with("vpnlane-") || n == "xtask")
        {
            seen += 1;
            if v != want {
                problems.push(format!("Cargo.lock {n} {v} != manifest {want}"));
            }
        }
    }
    if seen == 0 {
        problems.push("Cargo.lock has no workspace crate entries".into());
    }
    problems
}

/// Value of a `key = "value"` line.
fn quoted(key: &str, line: &str) -> Option<String> {
    let v = line
        .strip_prefix(key)?
        .trim_start()
        .strip_prefix('=')?
        .trim();
    Some(v.strip_prefix('"')?.strip_suffix('"')?.to_owned())
}

fn release(root: &Path) -> Result<()> {
    // cargo-dist (dist-workspace.toml) builds the shipped archives; this is the local check.
    let status = Command::new("cargo")
        .current_dir(root)
        .args([
            "build",
            "--release",
            "-p",
            "vpnlane-cli",
            "-p",
            "vpnlane-macos",
        ])
        .status()
        .wrap_err("running cargo build --release")?;
    if !status.success() {
        bail!("cargo build --release failed: {status}");
    }
    for bin in [brand_name!(), concat!(brand_name!(), "-helper")] {
        println!("{}", root.join("target/release").join(bin).display());
    }
    Ok(())
}

fn workspace_root() -> PathBuf {
    // xtask lives one level below the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

fn completions(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).wrap_err_with(|| format!("creating {}", dir.display()))?;
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let path =
            clap_complete::generate_to(shell, &mut vpnlane_cli::command(), brand_name!(), dir)?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}

fn man(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).wrap_err_with(|| format!("creating {}", dir.display()))?;
    clap_mangen::generate_to(vpnlane_cli::command(), dir)?;
    eprintln!("wrote man pages to {}", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check_versions, table_codes};

    const TOML: &str = "[workspace]\n[workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace.dependencies]\n";
    const LOCK: &str = "[[package]]\nname = \"vpnlane-cli\"\nversion = \"0.1.0\"\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n[[package]]\nname = \"xtask\"\nversion = \"0.1.0\"\n";
    const MANIFEST: &str = r#"{ ".": "0.1.0" }"#;

    #[test]
    fn continent_table_has_no_duplicate_codes() {
        let src = include_str!("../../crates/vpnlane-cli/src/tui/continent.rs");
        let codes = table_codes(src);
        assert!(codes.len() > 100, "table scan found {} codes", codes.len());
        let unique: std::collections::BTreeSet<_> = codes.iter().collect();
        assert_eq!(
            unique.len(),
            codes.len(),
            "duplicate code in continent table"
        );
    }

    #[test]
    fn agreeing_files_pass() {
        assert_eq!(check_versions(MANIFEST, TOML, LOCK), Vec::<String>::new());
    }

    #[test]
    fn each_disagreement_is_reported() {
        // Manifest moved: workspace version plus both lock entries lag.
        assert_eq!(check_versions(r#"{ ".": "0.2.0" }"#, TOML, LOCK).len(), 3);
        let stale = LOCK.replace("version = \"0.1.0\"\n", "version = \"0.0.9\"\n");
        assert_eq!(check_versions(MANIFEST, TOML, &stale).len(), 2);
        assert_eq!(check_versions("{}", TOML, LOCK).len(), 1);
        assert_eq!(check_versions(MANIFEST, TOML, "").len(), 1);
    }
}
