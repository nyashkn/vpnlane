//! `vpnlane alias [NAME]`: print a shell alias snippet. Never writes a file.

use clap::ValueEnum;
use color_eyre::{Result, eyre::bail};

use crate::context::Context;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Shell {
    Zsh,
    Bash,
    Fish,
}

#[derive(Debug, clap::Args)]
#[command(
    about = "Print a short shell alias (default: vl) with completions",
    long_about = concat!("\
Print a shell snippet that makes NAME an alias for ", brand_name!(), ", with completions.
It only prints to stdout; it never edits your shell config. Load the output yourself.
The snippet reuses the completions script, so load that too (see `", brand_name!(), " completions --help`)."),
    after_help = concat!("\
Examples:
  eval \"$(", brand_name!(), " alias)\"                  # try it in this shell
  echo \"$(", brand_name!(), " alias)\" >> ~/.zshrc      # keep it
  ", brand_name!(), " alias go --shell fish | source")
)]
pub(crate) struct Args {
    /// Alias name: letters, digits, `-` and `_`; must not start with `-`.
    #[arg(default_value = "vl", value_name = "NAME")]
    pub(crate) name: String,
    /// Shell syntax to print [default: from $SHELL, else zsh].
    #[arg(long, value_enum)]
    pub(crate) shell: Option<Shell>,
}

pub(crate) fn run(_ctx: &Context, args: &Args) -> Result<()> {
    let shell = args
        .shell
        .unwrap_or_else(|| detect(std::env::var("SHELL").ok().as_deref().unwrap_or_default()));
    print!("{}", render(&args.name, shell)?);
    Ok(())
}

fn detect(shell_env: &str) -> Shell {
    match shell_env.rsplit('/').next() {
        Some("bash") => Shell::Bash,
        Some("fish") => Shell::Fish,
        _ => Shell::Zsh,
    }
}

fn validate(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        bail!(
            "invalid alias name {name:?}: use letters, digits, '-' and '_', and do not start with '-'"
        );
    }
    if name == brand_name!() {
        bail!("{name:?} is already the command name; pick another alias");
    }
    Ok(())
}

fn render(name: &str, shell: Shell) -> Result<String> {
    validate(name)?;
    let bin = brand_name!();
    Ok(match shell {
        Shell::Zsh => format!(
            "# {bin} alias (printed by `{bin} alias`)\nalias {name}={bin}\n(( $+functions[compdef] )) && compdef {name}={bin}\n"
        ),
        Shell::Bash => format!(
            "# {bin} alias (printed by `{bin} alias`)\nalias {name}={bin}\ncomplete -F _{bin} -o bashdefault -o default {name}\n"
        ),
        Shell::Fish => format!(
            "# {bin} alias (printed by `{bin} alias`)\nalias {name} {bin}\ncomplete -c {name} -w {bin}\n"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_per_shell() {
        for (s, n) in [
            (Shell::Zsh, "zsh"),
            (Shell::Bash, "bash"),
            (Shell::Fish, "fish"),
        ] {
            insta::assert_snapshot!(format!("snippet_{n}"), render("vl", s).unwrap());
        }
    }

    #[test]
    fn rejects_bad_names() {
        for bad in ["", "-x", "a b", "a;b", "$(x)", "a/b", "vpnlane"] {
            assert!(render(bad, Shell::Zsh).is_err(), "{bad:?}");
        }
        assert!(render("go-2_x", Shell::Bash).is_ok());
    }

    #[test]
    fn detects_shell() {
        assert_eq!(detect("/bin/bash"), Shell::Bash);
        assert_eq!(detect("/usr/bin/fish"), Shell::Fish);
        assert_eq!(detect(""), Shell::Zsh);
    }
}
