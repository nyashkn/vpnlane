//! `cargo xtask <task>`: repo automation in Rust instead of shell scripts.

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use color_eyre::Result;
use color_eyre::eyre::{WrapErr, bail};

#[derive(Parser)]
#[command(about = "wglane development tasks")]
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
    /// Build release artifacts (not implemented yet).
    Release,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let out = workspace_root().join("target").join("assets");
    match Cli::parse().task {
        Task::Completions => completions(&out.join("completions")),
        Task::Man => man(&out.join("man")),
        Task::Release => {
            // TODO(bead): xtask-release: cargo-dist (or plain `cargo build --release`
            // + tarball + sha256) for aarch64/x86_64-apple-darwin, bundle
            // completions + man pages, emit a Homebrew formula for the tap.
            bail!("xtask release: not implemented yet")
        }
    }
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
        let path = clap_complete::generate_to(shell, &mut wglane_cli::command(), "wglane", dir)?;
        eprintln!("wrote {}", path.display());
    }
    Ok(())
}

fn man(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).wrap_err_with(|| format!("creating {}", dir.display()))?;
    clap_mangen::generate_to(wglane_cli::command(), dir)?;
    eprintln!("wrote man pages to {}", dir.display());
    Ok(())
}
