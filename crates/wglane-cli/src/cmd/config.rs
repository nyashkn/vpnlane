//! `wglane config`: where the settings file is, what is in effect, a sample.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use wglane_core::{Effective, Overrides, SAMPLE_CONFIG};

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Show or create the saved-settings file (config.toml)",
    long_about = "\
Saved settings live in <config dir>/config.toml (the directory that holds the
token; override it with --config-dir or WGLANE_CONFIG_DIR). A missing file
means the built-in defaults. Unknown keys are an error naming the key and line.

Precedence for each value: CLI flag > environment > config file > default.
The Nord token is never a config key.

Reference: docs/CONFIG.md.",
    after_help = "\
Examples:
  wglane config path       print the file location
  wglane config show       effective values and where each came from
  wglane config show --json
  wglane config init       write a commented sample (never overwrites)"
)]
pub(crate) struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// Print the path of config.toml (it may not exist yet).
    Path,
    /// Print every effective value with its source: flag, env, file or default.
    Show,
    /// Write a commented sample config.toml; refuses to overwrite.
    Init,
}

pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()> {
    match args.action {
        Action::Path => println!("{}", ctx.config_file().display()),
        // No flag layer here: flags belong to the commands that take them.
        Action::Show => println!("{}", show(&ctx.settings(Overrides::default())?, ctx.json)?),
        Action::Init => {
            let path = ctx.config_file();
            init(&path)?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// Creates `path` with the sample, never replacing an existing file.
fn init(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => eyre!(
                "{} already exists; edit it, or delete it to start over",
                path.display()
            ),
            _ => eyre!("cannot create {}: {e}", path.display()),
        })?;
    f.write_all(SAMPLE_CONFIG.as_bytes())?;
    Ok(())
}

fn show(e: &Effective, json: bool) -> Result<String> {
    if json {
        return Ok(serde_json::to_string_pretty(e)?);
    }
    let mut out = String::new();
    let mut row = |key: &str, value: String, source: &dyn std::fmt::Display| {
        let _ = writeln!(out, "{key:<22} {value:<16} {source}");
    };
    let opt = |s: Option<String>| s.unwrap_or_else(|| "(unset)".to_owned());
    row(
        "default_country",
        opt(e.default_country.value.map(|c| c.to_string())),
        &e.default_country.source,
    );
    row(
        "default_city",
        opt(e.default_city.value.clone()),
        &e.default_city.source,
    );
    row("pick (reserved)", e.pick.value.to_string(), &e.pick.source);
    let extra = &e.bypass_extra.value;
    let v = if extra.is_empty() {
        "[]".to_owned()
    } else {
        extra
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    row("bypass_extra", v, &e.bypass_extra.source);
    row(
        "exit_ip_check",
        e.exit_ip_check.value.to_string(),
        &e.exit_ip_check.source,
    );
    row(
        "tui.start_expanded",
        e.tui.start_expanded.value.to_string(),
        &e.tui.start_expanded.source,
    );
    row(
        "tui.graph_samples",
        e.tui.graph_samples.value.to_string(),
        &e.tui.graph_samples.source,
    );
    row(
        "tui.warn_ratio",
        format!("{:?}", e.tui.warn_ratio.value),
        &e.tui.warn_ratio.source,
    );
    Ok(out.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use wglane_core::FileConfig;

    use super::*;

    fn eff(toml: &str, env: Overrides) -> Effective {
        FileConfig::parse(toml)
            .unwrap()
            .resolve(Overrides::default(), env)
    }

    #[test]
    fn show_defaults_file_and_env() {
        insta::assert_snapshot!(
            "defaults",
            show(&eff("", Overrides::default()), false).unwrap()
        );
        let env = Overrides {
            country: Some("fr".parse().unwrap()),
            exit_ip_check: None,
        };
        let file = "default_country = \"DE\"\nexit_ip_check = false\npick = \"fastest\"\n\
                    bypass_extra = [\"203.0.113.0/24\"]\n[tui]\nwarn_ratio = 2.0\n";
        insta::assert_snapshot!("file_and_env", show(&eff(file, env), false).unwrap());
    }

    #[test]
    fn show_json_has_sources() {
        let j = show(&eff("exit_ip_check = false", Overrides::default()), true).unwrap();
        assert!(j.contains("\"source\": \"file\"") && j.contains("\"source\": \"default\""));
    }

    #[test]
    fn init_writes_sample_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("config.toml");
        init(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, SAMPLE_CONFIG);
        assert_eq!(FileConfig::parse(&text).unwrap(), FileConfig::default());
        let err = init(&path).unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SAMPLE_CONFIG);
    }
}
