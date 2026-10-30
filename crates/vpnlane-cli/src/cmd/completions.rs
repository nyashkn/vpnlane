//! `vpnlane completions <SHELL>`: print a shell completion script.

use std::io::Write;

use clap_complete::Shell;
use color_eyre::Result;

use crate::context::Context;

#[derive(Debug, clap::Args)]
#[command(
    about = "Print shell completions (zsh, bash, fish, ...)",
    long_about = "Print a completion script for SHELL to stdout.",
    after_help = concat!("Examples:
  ", brand_name!(), " completions zsh > \"${fpath[1]}/_", brand_name!(), "\"
  ", brand_name!(), " completions bash > ~/.local/share/bash-completion/completions/", brand_name!(), "
  ", brand_name!(), " completions fish > ~/.config/fish/completions/", brand_name!(), ".fish")
)]
pub(crate) struct Args {
    /// Shell to generate for.
    pub(crate) shell: Shell,
}

pub(crate) fn run(_ctx: &Context, args: &Args) -> Result<()> {
    let mut out = std::io::stdout().lock();
    write(args.shell, &mut out);
    out.flush()?;
    Ok(())
}

/// Writes the completion script for `shell` to `out`.
pub(crate) fn write(shell: Shell, out: &mut impl Write) {
    clap_complete::generate(shell, &mut crate::command(), brand_name!(), out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(shell: Shell) -> String {
        let mut buf = Vec::new();
        write(shell, &mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn zsh_covers_every_subcommand() {
        let s = script(Shell::Zsh);
        assert!(s.starts_with("#compdef vpnlane"), "{}", &s[..40]);
        for sub in crate::command().get_subcommands() {
            assert!(
                s.contains(sub.get_name()),
                "zsh completion lacks {}",
                sub.get_name()
            );
        }
    }

    #[test]
    fn bash_and_fish_generate() {
        assert!(script(Shell::Bash).contains("_vpnlane"));
        assert!(script(Shell::Fish).contains("complete -c vpnlane"));
    }
}
