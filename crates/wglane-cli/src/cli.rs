//! Top-level clap definition. Per-subcommand help text lives on each
//! `cmd::<name>::Args` struct, not here, so builders own their own file.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::cmd;

const HELP_TEMPLATE: &str = "\
{before-help}{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}";

#[derive(Debug, Parser)]
#[command(
    name = "wglane",
    version,
    about = "NordLynx (WireGuard) tunnels on macOS that leave Tailscale working",
    long_about = "\
NordLynx (WireGuard) tunnels on macOS that leave Tailscale/Headscale working.

A full WireGuard tunnel swallows Tailscale's own underlay traffic (control
server, DERP relays), so the tailnet dies. wglane brings the tunnel up and
routes that underlay around it via your physical gateway.",
    after_help = "\
Examples:
  wglane up ZA          connect to South Africa (switches if already up)
  wglane status         country, exit IP, tailnet check, bypass routes
  wglane down           disconnect and remove bypass routes
  wglane doctor         check prerequisites and print fixes
  wglane config show    saved settings in effect, and where each came from

Token: set WGLANE_TOKEN or write it to ~/.config/wglane/token (mode 600).",
    help_template = HELP_TEMPLATE
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) global: GlobalArgs,

    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Flags accepted by every subcommand.
#[derive(Debug, clap::Args)]
pub(crate) struct GlobalArgs {
    /// Machine-readable JSON output (for scripts and status bars).
    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// More log output: -v info, -vv debug, -vvv trace. `RUST_LOG` overrides.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// Config directory [default: ~/.config/wglane].
    #[arg(
        long,
        global = true,
        env = "WGLANE_CONFIG_DIR",
        hide_env_values = true,
        value_name = "DIR"
    )]
    pub(crate) config_dir: Option<PathBuf>,
}

// Variants carry no doc comments: help text comes from each Args struct.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Up(cmd::up::Args),
    Down(cmd::down::Args),
    Status(cmd::status::Args),
    Servers(cmd::servers::Args),
    Ls(cmd::ls::Args),
    Gen(cmd::gen_conf::Args),
    Doctor(cmd::doctor::Args),
    Config(cmd::config::Args),
    Watch(cmd::watch::Args),
    Tui(cmd::tui::Args),
    Completions(cmd::completions::Args),
    InstallHelper(cmd::install_helper::Args),
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn clap_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn top_level_help() {
        let help = Cli::command()
            .term_width(100)
            .render_long_help()
            .to_string()
            // The release flow bumps the version; the snapshot must not.
            .replace(env!("CARGO_PKG_VERSION"), "X.Y.Z");
        insta::assert_snapshot!(help);
    }

    #[test]
    fn every_subcommand_has_examples() {
        for sub in Cli::command().get_subcommands() {
            let after = sub
                .get_after_help()
                .map(ToString::to_string)
                .unwrap_or_default();
            assert!(
                after.contains("wglane"),
                "`{}` needs after_help examples",
                sub.get_name()
            );
            assert!(
                sub.get_about().is_some(),
                "`{}` needs about",
                sub.get_name()
            );
        }
    }

    #[test]
    fn country_is_validated_by_clap() {
        assert!(Cli::try_parse_from(["wglane", "up", "za"]).is_ok());
        assert!(Cli::try_parse_from(["wglane", "up", "zaf"]).is_err());
    }
}
