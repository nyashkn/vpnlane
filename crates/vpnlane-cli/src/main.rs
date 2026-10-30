//! `vpnlane` binary. Everything lives in the library so xtask can reuse the
//! clap definition for man pages and completions.

fn main() -> color_eyre::Result<()> {
    vpnlane_cli::run()
}
