//! The `wglane` command line: argument parsing, dispatch, output.
//!
//! One file per subcommand in `cmd/`; this file only wires them up.

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
    color_eyre::install()?;
    let cli = Cli::parse();
    init_tracing(cli.global.verbose);
    let ctx = Context::from_global(&cli.global)?;
    match &cli.command {
        Command::Up(a) => cmd::up::run(&ctx, a),
        Command::Down(a) => cmd::down::run(&ctx, a),
        Command::Status(a) => cmd::status::run(&ctx, a),
        Command::Servers(a) => cmd::servers::run(&ctx, a),
        Command::Ls(a) => cmd::ls::run(&ctx, a),
        Command::Gen(a) => cmd::gen_conf::run(&ctx, a),
        Command::Doctor(a) => cmd::doctor::run(&ctx, a),
        Command::Config(a) => cmd::config::run(&ctx, a),
        Command::Watch(a) => cmd::watch::run(&ctx, a),
        Command::Tui(a) => cmd::tui::run(&ctx, a),
        Command::Completions(a) => cmd::completions::run(&ctx, a),
        Command::InstallHelper(a) => cmd::install_helper::run(&ctx, a),
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    // RUST_LOG, when set, wins over -v.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
