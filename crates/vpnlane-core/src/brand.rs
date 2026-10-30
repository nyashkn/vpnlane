//! Every product name in one place: a rename is the two macros below.
//!
//! Literals that cannot be a `const` (clap attributes, `concat!` help text)
//! use the macros; everything else uses the consts. A test in this module
//! fails if a raw product-name literal reappears in non-test source.

/// The product, binary and config-dir name.
#[macro_export]
macro_rules! brand_name {
    () => {
        "wglane"
    };
}

/// An environment variable name under the product prefix: `brand_env!("PICK")`.
#[macro_export]
macro_rules! brand_env {
    ($k:literal) => {
        concat!("WGLANE_", $k)
    };
}

/// Binary name.
pub const BIN: &str = brand_name!();
/// Root-side helper binary name (must match the Cargo `[[bin]]`).
pub const HELPER_BIN: &str = concat!(brand_name!(), "-helper");
/// Directory name under `~/.config`.
pub const CONFIG_DIR_NAME: &str = brand_name!();
/// Root-owned install path of the helper.
pub const HELPER_PATH: &str = concat!("/usr/local/libexec/", brand_name!(), "-helper");
/// Root-owned directory of vetted `wg`/`wireguard-go` copies.
pub const TOOL_DIR: &str = concat!("/usr/local/libexec/", brand_name!(), "/bin");
/// Root-owned runtime state directory.
pub const STATE_DIR: &str = concat!("/var/db/", brand_name!());
/// Sudoers drop-in path.
pub const SUDOERS_PATH: &str = concat!("/etc/sudoers.d/", brand_name!());
/// Name file wireguard-go is told to write (`WG_TUN_NAME_FILE`).
pub const WG_NAME_FILE: &str = concat!(brand_name!(), ".name");
/// The hint printed when the root helper is missing.
pub const INSTALL_HINT: &str = concat!("sudo ", brand_name!(), " install-helper");
/// GitHub repository slug.
pub const REPO_SLUG: &str = "nyashkn/wglane";

/// Environment variable name for `key` (`"COUNTRY"` -> `WGLANE_COUNTRY`).
pub fn env(key: &str) -> String {
    format!("{}{key}", brand_env!(""))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                if !matches!(name.as_str(), "tests" | "snapshots" | "target") {
                    rs_files(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") && name != "brand.rs" {
                out.push(p);
            }
        }
    }

    #[test]
    fn env_helper_uses_the_prefix() {
        assert_eq!(super::env("COUNTRY"), brand_env!("COUNTRY"));
    }

    /// No raw product-name literal in non-test source: use `brand`.
    #[test]
    fn no_raw_product_name_literals_outside_brand() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        for d in ["crates", "xtask"] {
            rs_files(&root.join(d), &mut files);
        }
        assert!(files.len() > 20, "scan found too few files");
        let mut bad = Vec::new();
        for f in files {
            let src = std::fs::read_to_string(&f).unwrap();
            let body = src.split("#[cfg(test)]").next().unwrap();
            for (i, line) in body.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || !t.contains('"') && !t.contains("WGLANE_") {
                    continue;
                }
                // Cargo package names, crate paths and `use` lines are not branding.
                let l = line
                    .replace("vpnlane-core", "")
                    .replace("wglane-cli", "")
                    .replace("vpnlane-macos", "")
                    .replace("vpnlane-nord", "")
                    .replace("\"wglane-\"", "")
                    .replace("wglane_", "");
                let in_str = l.contains("WGLANE_") || l.contains("wglane");
                if in_str && l.matches('"').count() >= 1 {
                    bad.push(format!("{}:{}: {}", f.display(), i + 1, line.trim()));
                }
            }
        }
        assert!(bad.is_empty(), "raw product name in:\n{}", bad.join("\n"));
    }
}
