//! The `vpnlane` command line: argument parsing, dispatch, output.
//!
//! One file per subcommand in `cmd/`; this file only wires them up.

#[macro_use]
extern crate vpnlane_core;

mod cli;
mod cmd;
mod context;
#[cfg(feature = "tui")]
mod tui;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use context::Context;

/// The full clap command tree, for completions and man pages.
pub fn command() -> clap::Command {
    Cli::command()
}

/// Parses argv, sets up error reporting and logging, runs the subcommand.
pub fn run() -> color_eyre::Result<()> {
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        // The blank theme: plain-text error reports.
        color_eyre::config::HookBuilder::new()
            .theme(color_eyre::config::Theme::new())
            .install()?;
    } else {
        color_eyre::install()?;
    }
    let cli = Cli::parse();
    let ctx = Context::from_global(&cli.global)?;
    // The dashboard owns the terminal: a log line on stderr would paint over it.
    let log_file = matches!(cli.command, Command::Tui(_)).then(|| ctx.config_dir.join("tui.log"));
    init_tracing(cli.global.verbose, log_file.as_deref());
    match &cli.command {
        Command::Up(a) => cmd::up::run(&ctx, a),
        Command::Down(a) => cmd::down::run(&ctx, a),
        Command::Status(a) => cmd::status::run(&ctx, a),
        Command::Servers(a) => cmd::servers::run(&ctx, a),
        Command::Ls(a) => cmd::ls::run(&ctx, a),
        Command::Preset(a) => cmd::preset::run(&ctx, a),
        Command::Gen(a) => cmd::gen_conf::run(&ctx, a),
        Command::Doctor(a) => cmd::doctor::run(&ctx, a),
        Command::Config(a) => cmd::config::run(&ctx, a),
        Command::Watch(a) => cmd::watch::run(&ctx, a),
        Command::Tui(a) => cmd::tui::run(&ctx, a),
        Command::Completions(a) => cmd::completions::run(&ctx, a),
        Command::InstallHelper(a) => cmd::install_helper::run(&ctx, a),
    }
}

/// Logs go to stderr, or to `log_file` (appended; dropped if it cannot be
/// opened) while the TUI holds the screen.
fn init_tracing(verbose: u8, log_file: Option<&std::path::Path>) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    // RUST_LOG, when set, wins over -v.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    let fmt = tracing_subscriber::fmt().with_env_filter(filter);
    match log_file {
        None => fmt.with_writer(std::io::stderr).init(),
        Some(path) => {
            let file = std::fs::create_dir_all(path.parent().unwrap_or(path))
                .and_then(|()| {
                    // 0600: log lines can carry IPs and hostnames.
                    std::os::unix::fs::OpenOptionsExt::mode(
                        std::fs::OpenOptions::new().create(true).append(true),
                        0o600,
                    )
                    .open(path)
                })
                .ok();
            let sink: Box<dyn std::io::Write + Send> = match file {
                Some(f) => Box::new(f),
                None => Box::new(std::io::sink()),
            };
            fmt.with_ansi(false)
                .with_writer(std::sync::Mutex::new(sink))
                .init();
        }
    }
}
